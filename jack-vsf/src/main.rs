use jack::{
    AudioIn, AudioOut, Client, ClientOptions, Control, Frames, Port, ProcessHandler, ProcessScope,
};
use std::env::args;
use std::fs::File;
use virtual_surround::{get_channel_name, RawVirtualSurroundFilter};

struct Filter {
    vsf: RawVirtualSurroundFilter,
    input_ports: Vec<Port<AudioIn>>,
    input_space: Vec<Vec<f32>>,
    input_offset: usize,
    buffer_size: usize,
    output_ports: Vec<Port<AudioOut>>,
    output_buffer: usize,
    output_space: Vec<Vec<f32>>,
    has_buffer: bool,
}

fn main() -> anyhow::Result<()> {
    let args = args().collect::<Vec<String>>();
    if args.len() < 2 {
        println!("usage: {} <hrir file>", &args[0]);
        return Ok(());
    }

    let file = File::open(&args[1])?;

    let (client, _) = Client::new(
        "Virtual Surround",
        ClientOptions::USE_EXACT_NAME | ClientOptions::NO_START_SERVER,
    )?;

    let vsf = RawVirtualSurroundFilter::new(file, Some(client.sample_rate() as u32))?;

    println!(
        "forced latency of {} samples / {} ms",
        vsf.sample_latency(),
        vsf.sample_latency() as f32 / (vsf.sample_rate() / 1000) as f32
    );

    let mut input_ports = vec![];

    let mut input_space = vec![];

    for chan in vsf.positions() {
        let port = client.register_port(&format!("input_{}", get_channel_name(chan)), AudioIn)?;
        input_ports.push(port);
        input_space.push(vec![0f32; vsf.samples_required()]);
    }

    let mut output_ports = vec![];
    output_ports.push(client.register_port("output_FL", AudioOut)?);
    output_ports.push(client.register_port("output_FR", AudioOut)?);

    let block_size = vsf.block_size();
    client.set_buffer_size(block_size as u32)?;

    let client = client.activate_async(
        (),
        Filter {
            vsf,
            input_ports,
            input_space,
            input_offset: 0,
            buffer_size: block_size,
            output_buffer: 0,
            output_ports,
            output_space: vec![vec![0f32; block_size], vec![0f32; block_size]],
            has_buffer: false,
        },
    )?;

    std::io::stdin().read_line(&mut String::new())?;

    client.deactivate()?;

    Ok(())
}

impl ProcessHandler for Filter {
    fn process(&mut self, client: &Client, process_scope: &ProcessScope) -> Control {
        if process_scope.n_frames() as usize != self.buffer_size {
            if self.buffer_size(client, process_scope.n_frames()) == Control::Quit {
                return Control::Quit;
            }
        }

        for (c, port) in self.input_ports.iter().enumerate() {
            self.input_space[c][self.input_offset..self.input_offset + self.buffer_size]
                .copy_from_slice(port.as_slice(process_scope));
        }

        if self.input_offset < (self.vsf.samples_required() - self.buffer_size) {
            self.input_offset += self.buffer_size;
            if self.has_buffer && self.output_buffer < self.vsf.block_size() {
                self.output_ports[0]
                    .as_mut_slice(process_scope)
                    .copy_from_slice(
                        &self.output_space[0]
                            [self.output_buffer..self.output_buffer + self.buffer_size],
                    );
                self.output_ports[1]
                    .as_mut_slice(process_scope)
                    .copy_from_slice(
                        &self.output_space[1]
                            [self.output_buffer..self.output_buffer + self.buffer_size],
                    );
                self.output_buffer += self.buffer_size;

                if self.output_buffer >= self.vsf.block_size() {
                    self.has_buffer = false;
                }
            }

            return Control::Continue;
        }

        let mut output_buffers = if self.buffer_size == self.vsf.block_size() {
            self.output_ports
                .iter_mut()
                .map(|x| x.as_mut_slice(process_scope))
                .collect::<Vec<_>>()
        } else {
            self.output_space
                .iter_mut()
                .map(|x| x.as_mut_slice())
                .collect::<Vec<_>>()
        };

        let left = output_buffers.remove(0);
        let right = output_buffers.remove(0);

        left.fill(0.0);
        right.fill(0.0);

        // what errors?
        let _ = self.vsf.transform(
            &mut self
                .input_space
                .iter_mut()
                .map(|x| x.as_mut_slice())
                .collect::<Vec<_>>(),
            (left, right),
        );

        if self.buffer_size != self.vsf.block_size() {
            self.output_ports[0]
                .as_mut_slice(process_scope)
                .copy_from_slice(&self.output_space[0][..self.buffer_size]);
            self.output_ports[1]
                .as_mut_slice(process_scope)
                .copy_from_slice(&self.output_space[1][..self.buffer_size]);
            self.output_buffer = self.buffer_size;
            self.has_buffer = true;
        }

        for space in &mut self.input_space {
            space.copy_within(self.vsf.block_size().., 0);
        }

        self.input_offset = self.vsf.samples_required() - self.vsf.block_size();

        Control::Continue
    }

    fn buffer_size(&mut self, _: &Client, size: Frames) -> Control {
        if size as usize == self.buffer_size {
            return Control::Continue;
        }

        if self.vsf.block_size() % size as usize != 0 || size as usize > self.vsf.block_size() {
            println!("JACK buffer size needs to be equal or smaller and (buffer_size % block_size) === 0, requested buffer size is {}, block size is {}", size, self.vsf.block_size());
            return Control::Quit;
        }

        println!("Buffer size changed from {} to {}", self.buffer_size, size);
        self.buffer_size = size as usize;
        self.input_offset = 0;
        self.has_buffer = false;
        Control::Continue
    }
}
