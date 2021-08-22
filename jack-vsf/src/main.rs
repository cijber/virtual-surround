use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};
use std::env::args;
use std::fs::File;
use virtual_surround::{get_channel_name, RawVirtualSurroundFilter};

struct Filter {
    vsf: RawVirtualSurroundFilter,
    input_ports: Vec<Port<AudioIn>>,
    input_space: Vec<Vec<f32>>,
    input_offset: usize,
    output_ports: Vec<Port<AudioOut>>,
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

    client.set_buffer_size(vsf.block_size() as u32)?;

    let client = client.activate_async(
        (),
        Filter {
            vsf,
            input_ports,
            input_space,
            input_offset: 0,
            output_ports,
        },
    )?;

    std::io::stdin().read_line(&mut String::new())?;

    client.deactivate()?;

    Ok(())
}

impl ProcessHandler for Filter {
    fn process(&mut self, _: &Client, process_scope: &ProcessScope) -> Control {
        for (c, port) in self.input_ports.iter().enumerate() {
            self.input_space[c][self.input_offset..self.input_offset + self.vsf.block_size()]
                .copy_from_slice(port.as_slice(process_scope));
        }

        if self.input_offset < self.vsf.sample_latency() {
            self.input_offset += self.vsf.block_size();

            return Control::Continue;
        }

        let mut output_buffers = self
            .output_ports
            .iter_mut()
            .map(|x| x.as_mut_slice(process_scope))
            .collect::<Vec<_>>();

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

        for space in &mut self.input_space {
            space.copy_within(self.vsf.block_size().., 0);
        }

        Control::Continue
    }
}
