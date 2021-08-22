use bwavfile::{CommonFormat, WaveFmt, WaveReader};
use std::io::{Read, Seek};

pub use bwavfile::ChannelMask;
use std::convert::{TryFrom, TryInto};
use std::fmt::{Debug, Formatter};

#[cfg(feature = "rustfft")]
mod rustfft;

#[cfg(feature = "rustfft")]
pub use crate::rustfft::*;
use anyhow::Context;
use samplerate::ConverterType;

// "biggest" surround sound system is 22.2
// so 24 should be enough, for now
pub const MAX_CHANNELS: usize = 24;

pub const BLOCK_SIZE: usize = 512;

#[derive(Debug, Copy, Clone)]
pub enum SampleFormat {
    F32,
}

pub fn mirror_channel(channel: ChannelMask) -> ChannelMask {
    match channel {
        ChannelMask::FrontLeft => ChannelMask::FrontRight,
        ChannelMask::FrontRight => ChannelMask::FrontLeft,
        ChannelMask::BackLeft => ChannelMask::BackRight,
        ChannelMask::BackRight => ChannelMask::BackLeft,
        ChannelMask::FrontCenterLeft => ChannelMask::FrontCenterRight,
        ChannelMask::FrontCenterRight => ChannelMask::FrontCenterLeft,
        ChannelMask::SideLeft => ChannelMask::SideRight,
        ChannelMask::SideRight => ChannelMask::SideLeft,
        ChannelMask::TopFrontLeft => ChannelMask::TopFrontRight,
        ChannelMask::TopFrontRight => ChannelMask::TopFrontLeft,
        ChannelMask::TopBackLeft => ChannelMask::TopBackRight,
        ChannelMask::TopBackRight => ChannelMask::TopBackLeft,

        // center channels
        center => center,
    }
}

impl TryFrom<WaveFmt> for SampleFormat {
    type Error = anyhow::Error;

    fn try_from(value: WaveFmt) -> Result<Self, Self::Error> {
        match (value.common_format(), value.bits_per_sample) {
            (CommonFormat::IeeeFloatPCM, 32) => Ok(SampleFormat::F32),
            (format, bits) => {
                anyhow::bail!(
                    "VirtualSurround doesn't currently support {:?} at {} bits",
                    format,
                    bits
                );
            }
        }
    }
}

#[derive(Copy, Clone)]
struct ChannelMap {
    channels: usize,
    map: [ChannelMask; MAX_CHANNELS],
}

pub fn get_channel_name(mask: ChannelMask) -> &'static str {
    match mask {
        ChannelMask::DirectOut => "NA",
        ChannelMask::FrontLeft => "FL",
        ChannelMask::FrontRight => "FR",
        ChannelMask::FrontCenter => "FC",
        ChannelMask::LowFrequency => "LFE",
        ChannelMask::BackLeft => "RL",
        ChannelMask::BackRight => "RR",
        ChannelMask::FrontCenterLeft => "FLC",
        ChannelMask::FrontCenterRight => "FRC",
        ChannelMask::BackCenter => "RC",
        ChannelMask::SideLeft => "SL",
        ChannelMask::SideRight => "SR",
        ChannelMask::TopCenter => "TC",
        ChannelMask::TopFrontLeft => "TFL",
        ChannelMask::TopFrontCenter => "TFC",
        ChannelMask::TopFrontRight => "TFR",
        ChannelMask::TopBackLeft => "TRL",
        ChannelMask::TopBackCenter => "TRC",
        ChannelMask::TopBackRight => "RTR",
    }
}

impl ChannelMap {
    pub fn from_iter<I: Iterator<Item = ChannelMask>>(iter: I) -> anyhow::Result<ChannelMap> {
        let mut channels: usize = 0;
        let mut map = [ChannelMask::DirectOut; MAX_CHANNELS];

        for mask in iter {
            if channels >= MAX_CHANNELS {
                anyhow::bail!(
                    "Iterator returns more channels than supported ({})",
                    MAX_CHANNELS
                );
            }

            map[channels] = mask;
            channels += 1;
        }

        Ok(ChannelMap { channels, map })
    }

    pub fn find(&self, channel: ChannelMask) -> Option<usize> {
        for i in 0..self.channels {
            if self.map[i] == channel {
                return Some(i);
            }
        }

        None
    }

    pub fn find_mirror(&self, channel: ChannelMask) -> Option<usize> {
        self.find(mirror_channel(channel))
    }
}

impl Debug for ChannelMap {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelMap")
            .field("channels", &self.channels)
            .field("map", &self.map[..self.channels].to_vec())
            .finish()
    }
}

#[derive(Debug)]
pub struct VirtualSurroundFilter<T: FFTLogic = CurrentFFTLogic> {
    inner: RawVirtualSurroundFilter<T>,
    available_data: usize,
    left_out_space: Vec<f32>,
    right_out_space: Vec<f32>,
    in_space: [Vec<f32>; MAX_CHANNELS],
}

#[derive(Debug)]
pub struct RawVirtualSurroundFilter<T: FFTLogic = CurrentFFTLogic> {
    channel_map: ChannelMap,
    rate: usize,
    format: SampleFormat,
    fft_logic: T,
    fft_len: usize,
    rev_space: Vec<f32>,
}

impl RawVirtualSurroundFilter {
    pub fn new<R: Read + Seek>(reader: R, sample_rate: Option<u32>) -> anyhow::Result<Self> {
        if !cfg!(feature = "resample") && sample_rate.is_some() {
            panic!("virtual-surround is compiled without resampling support, cannot request resampling");
        }

        let mut item = WaveReader::new(reader)?;

        let channels = item.channels()?;

        if channels.len() > MAX_CHANNELS {
            anyhow::bail!("Input HRIR file has {} channels, VirtualSurroundFilter is compiled with only support for max {} channels", channels.len(), MAX_CHANNELS);
        }

        let fmt = item.format()?;
        let mut reader = item.audio_frame_reader()?;
        let mut buffer = [0f32; MAX_CHANNELS];

        let mut data = Vec::new();

        let mut samples = 0;
        while let Ok(1) = reader.read_float_frame(&mut buffer[..channels.len()]) {
            data.extend_from_slice(&buffer[..channels.len()]);
            samples += 1;
        }

        let mut current_rate = fmt.sample_rate;

        #[cfg(feature = "resample")]
        {
            if let Some(target_sample_rate) = sample_rate {
                if target_sample_rate != fmt.sample_rate {
                    data = samplerate::convert(
                        fmt.sample_rate,
                        target_sample_rate as u32,
                        channels.len(),
                        ConverterType::SincBestQuality,
                        &data,
                    )?;

                    samples = data.len() / channels.len();

                    current_rate = target_sample_rate;
                }
            }
        }

        normalize_hrir(&mut data, samples, channels.len());

        let fft_len: usize = {
            let goal = samples + BLOCK_SIZE + 1;
            let mut i = 5;
            let mut m = 0usize;
            while m < goal {
                i += 1;
                m = 2usize.pow(i);
            }

            m
        };

        let channel_map = ChannelMap::from_iter(channels.iter().map(|x| x.speaker))?;

        let mut fft_logic: CurrentFFTLogic = FFTLogic::new(channels.len(), fft_len);

        let rev_space = vec![0f32; fft_len];

        let mut channels_left = [0; MAX_CHANNELS];
        let mut channels_right = [0; MAX_CHANNELS];

        for i in 0..channel_map.channels {
            channels_left[i] = i;
            channels_right[i] = channel_map
                .find_mirror(channel_map.map[i])
                .with_context(|| {
                    format!(
                        "hrir file isn't symmetrical can't find the mirrored side of {:?}",
                        channel_map.map[i]
                    )
                })?;
        }

        let mut impulse_temp = vec![0f32; fft_len];

        for i in 0..channels.len() {
            for ear in [0, 1] {
                let index = (i * 2) + ear;
                let impulse_index = if ear == 0 {
                    channels_left[i]
                } else {
                    channels_right[i]
                };

                for j in 0..samples {
                    impulse_temp[j] = data[(j * channels.len()) + impulse_index];
                }

                fft_logic.init_ir(&mut impulse_temp, index)?;
            }
        }

        Ok(RawVirtualSurroundFilter {
            channel_map,
            rate: current_rate as usize,
            format: fmt.try_into()?,
            fft_logic,
            fft_len,
            rev_space,
        })
    }

    pub fn transform(
        &mut self,
        input: &mut [&mut [f32]],
        output: (&mut [f32], &mut [f32]),
    ) -> anyhow::Result<()> {
        for channel in 0..self.channel_map.channels {
            self.fft_logic.process_channel(
                channel,
                &mut input[channel],
                &mut self.rev_space,
                output.0,
                output.1,
            )?;
        }

        Ok(())
    }

    pub fn samples_required(&self) -> usize {
        self.fft_len
    }

    pub fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    pub fn sample_latency(&self) -> usize {
        self.fft_len - BLOCK_SIZE
    }

    pub fn sample_rate(&self) -> usize {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.channel_map.channels
    }

    pub fn positions(&self) -> impl Iterator<Item = ChannelMask> + '_ {
        self.channel_map.map[..self.channels()].iter().copied()
    }
}

impl VirtualSurroundFilter {
    #[cfg(feature = "resample")]
    pub fn new_from_hrir_and_sample_rate<R: Read + Seek>(
        reader: R,
        sample_rate: u32,
    ) -> anyhow::Result<Self> {
        Self::new(reader, Some(sample_rate))
    }

    pub fn new_from_hrir<R: Read + Seek>(reader: R) -> anyhow::Result<Self> {
        Self::new(reader, None)
    }

    fn new<R: Read + Seek>(reader: R, sample_rate: Option<u32>) -> anyhow::Result<Self> {
        let inner = RawVirtualSurroundFilter::new(reader, sample_rate)?;

        const EMPTY_VEC: Vec<f32> = Vec::new();
        let mut in_space = [EMPTY_VEC; MAX_CHANNELS];
        for i in 0..inner.channels() {
            in_space[i] = vec![0f32; inner.samples_required()];
        }

        let left_out_space = vec![0f32; inner.block_size() * 4];
        let right_out_space = vec![0f32; inner.block_size() * 4];

        let filter = VirtualSurroundFilter {
            inner,
            available_data: 0,
            left_out_space,
            right_out_space,
            in_space,
        };

        Ok(filter)
    }

    pub fn samples_required(&self) -> usize {
        self.inner.samples_required()
    }

    pub fn block_size(&self) -> usize {
        self.inner.block_size()
    }

    pub fn sample_latency(&self) -> usize {
        self.inner.sample_latency()
    }

    pub fn sample_rate(&self) -> usize {
        self.inner.sample_rate()
    }

    pub fn channels(&self) -> usize {
        self.inner.channels()
    }

    pub fn positions(&self) -> impl Iterator<Item = ChannelMask> + '_ {
        self.inner.positions()
    }

    pub fn transform(&mut self, input: &[f32], output: &mut [f32]) -> anyhow::Result<()> {
        let sample_count = input.len() / self.channels();
        let move_data = if self.available_data + sample_count > self.samples_required() {
            self.available_data = self.samples_required() - sample_count;
            sample_count
        } else {
            0
        };

        for c in 0..self.channels() {
            if move_data > 0 {
                self.in_space[c].copy_within(move_data.., 0);
            }

            for s in 0..sample_count {
                self.in_space[c][self.available_data + s] = input[s * self.channels() + c];
            }
        }

        self.available_data += sample_count;

        if self.available_data < self.samples_required() {
            return Ok(());
        }

        self.left_out_space.fill(0f32);
        self.right_out_space.fill(0f32);

        let left = &mut self.left_out_space;
        let right = &mut self.right_out_space;

        self.inner.transform(
            &mut self
                .in_space
                .iter_mut()
                .map(|x| x.as_mut_slice())
                .collect::<Vec<_>>(),
            (left, right),
        )?;

        for s in 0..BLOCK_SIZE {
            let mut sample = self.left_out_space[s];
            if sample > 1.0 {
                sample = 1.0;
            }

            if sample < -1.0 {
                sample = -1.0;
            }
            output[s * 2] = sample;

            let mut sample = self.right_out_space[s];
            if sample > 1.0 {
                sample = 1.0;
            }

            if sample < -1.0 {
                sample = -1.0;
            }
            output[s * 2 + 1] = sample;
        }

        Ok(())
    }
}

/// from https://github.com/pulseaudio/pulseaudio/blob/19adddee31ca34bf4e0db95df01b4ec595f2d267/src/modules/module-virtual-surround-sink.c#L192
fn normalize_hrir(data: &mut [f32], samples: usize, channels: usize) {
    let scaling_factor = 2.5f32;

    let mut hrir_max: f32 = 0.0;

    for i in 0..samples {
        let mut hrir_sum = 0.0;
        for c in 0..channels {
            hrir_sum += data[i * channels + c].abs();
        }

        if hrir_sum > hrir_max {
            hrir_max = hrir_sum;
        }
    }

    for i in 0..samples {
        for c in 0..channels {
            data[i * channels + c] /= hrir_max * scaling_factor;
        }
    }
}

pub trait FFTLogic: Sized {
    fn new(channels: usize, length: usize) -> Self;

    fn init_ir(&mut self, impulse: &mut [f32], ir_index: usize) -> anyhow::Result<()>;

    fn process_channel(
        &mut self,
        channel: usize,
        samples: &mut [f32],
        rev_space: &mut [f32],
        left_output: &mut [f32],
        right_output: &mut [f32],
    ) -> anyhow::Result<()>;
}

#[cfg(feature = "rustfft")]
pub type CurrentFFTLogic = rustfft::RustFFTLogic;

#[cfg(test)]
mod tests {
    use crate::VirtualSurroundFilter;
    use std::fs::File;

    #[test]
    pub fn simple_passthrough() {
        let filter = VirtualSurroundFilter::new_from_hrir(
            File::open("../resources/hrir_kemar/hrir-kemar.wav").unwrap(),
        )
        .unwrap();

        println!("{:#?}", filter)
    }
}
