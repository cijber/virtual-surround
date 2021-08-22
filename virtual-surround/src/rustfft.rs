#![cfg(feature = "rustfft")]

use crate::{FFTLogic, BLOCK_SIZE, MAX_CHANNELS};
use anyhow::Context;
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, ComplexToRealEven, RealToComplex, RealToComplexEven};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use std::fmt::{Debug, Formatter};

pub struct RustFFTLogic {
    length: usize,
    length_if: f32,
    input: Vec<Complex32>,
    output: Vec<Complex32>,
    ir: [Vec<Complex32>; MAX_CHANNELS * 2],
    forward_plan: RealToComplexEven<f32>,
    backward_plan: ComplexToRealEven<f32>,
    pub forward_scratch: Vec<Complex<f32>>,
    pub backward_scratch: Vec<Complex<f32>>,
}

impl Debug for RustFFTLogic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustFFTLogic")
            .field("input", &self.input)
            .field("output", &self.output)
            .field("ir", &self.ir)
            .finish_non_exhaustive()
    }
}

impl FFTLogic for RustFFTLogic {
    fn new(channels: usize, length: usize) -> Self {
        let input = vec![Complex32::default(); (length / 2) + 1];
        let output = vec![Complex32::default(); (length / 2) + 1];

        const EMPTY: Vec<Complex32> = Vec::new();
        let mut ir = [EMPTY; MAX_CHANNELS * 2];

        for i in 0..(channels * 2) {
            ir[i] = vec![Complex32::new(0f32, 0f32); (length / 2) + 1];
        }

        let mut planner = FftPlanner::<f32>::new();

        let forward_plan = RealToComplexEven::new(length, &mut planner);
        let backward_plan = ComplexToRealEven::new(length, &mut planner);

        let backward_scratch = backward_plan.make_scratch_vec();
        let forward_scratch = forward_plan.make_scratch_vec();

        RustFFTLogic {
            length,
            length_if: 1.0 / length as f32,
            input,
            output,
            ir,
            forward_plan,
            forward_scratch,
            backward_plan,
            backward_scratch,
        }
    }

    fn init_ir(&mut self, impulse: &mut [f32], ir_index: usize) -> anyhow::Result<()> {
        self.forward_plan
            .process_with_scratch(impulse, &mut self.ir[ir_index], &mut self.forward_scratch)
            .map_err(|err| anyhow::Error::msg(err.to_string()))
            .context("Failed to process IR")?;
        Ok(())
    }

    fn process_channel(
        &mut self,
        channel: usize,
        samples: &mut [f32],
        rev_space: &mut [f32],
        left_output: &mut [f32],
        right_output: &mut [f32],
    ) -> anyhow::Result<()> {
        self.forward_plan
            .process_with_scratch(samples, &mut self.input, &mut self.forward_scratch)
            .map_err(|err| anyhow::Error::msg(err.to_string()))
            .context("Failed to process channel")?;

        for ear in 0..2 {
            let ir = &mut self.ir[channel * 2 + ear];
            let out_space = if ear == 0 {
                &mut *left_output
            } else {
                &mut *right_output
            };

            for s in 0..(self.length / 2) + 1 {
                let re = ir[s].re * self.input[s].re - ir[s].im * self.input[s].im;
                let im = ir[s].im * self.input[s].re + ir[s].re * self.input[s].im;

                self.output[s] = Complex32::new(re, im);
            }

            self.backward_plan
                .process_with_scratch(&mut self.output, rev_space, &mut self.backward_scratch)
                .map_err(|err| anyhow::Error::msg(err.to_string()))
                .context("Failed to process channel")?;

            for s in 0..BLOCK_SIZE {
                out_space[s] += rev_space[(self.length - BLOCK_SIZE) + s] * self.length_if;
            }
        }

        Ok(())
    }
}
