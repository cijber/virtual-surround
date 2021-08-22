use hound::{SampleFormat, WavSpec};
use std::env::args;
use std::fs::File;
use virtual_surround::VirtualSurroundFilter;

pub fn main() {
    let arg = args().collect::<Vec<String>>();
    if arg.len() < 3 {
        println!("{} <input> <output>", arg[0]);
    }

    let r = bwavfile::WaveReader::open(&arg[1]).expect("Failed to open input wav");
    let spec = WavSpec {
        channels: 2,
        sample_rate: 44100,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut w = hound::WavWriter::create(&arg[2], spec).expect("Failed to create wav writer");

    let mut vs = VirtualSurroundFilter::new_from_hrir(
        File::open("resources/hrir_kemar/hrir-kemar.wav").expect("Failed to open hrir"),
    )
    .expect("Failed to create filter");
    let mut block: Vec<f32> = vec![0f32; vs.block_size() * 6];
    let mut offset = 0;

    let mut samples = vec![0f32; 6];

    let mut fr = r.audio_frame_reader().unwrap();

    while let Ok(1) = fr.read_float_frame(&mut samples) {
        block[offset..offset + samples.len()].copy_from_slice(&samples);
        offset += samples.len();

        if offset >= block.len() {
            println!("got full block");
            let mut output: Vec<f32> = vec![0f32; vs.block_size() * 2];
            vs.transform(&block, &mut output)
                .expect("Failed to transform");

            for sample in output {
                w.write_sample(sample).expect("Failed to write sample");
            }

            offset = 0;
        }
    }

    w.flush().expect("Failed to flush");
    w.finalize().expect("Failed to finalize");
}
