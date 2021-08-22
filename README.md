# Virtual Surround

A Rust crate which allows you to simulate surround sound in a stereo space, alternatively known as "_discount Dolby Atmos_" or "_eater's Shadow & Knuckles_" (MSFT's built-in version is called _Microsoft Sonic_)

This crate will _always_ introduce a small amount of delay which is needed for fourier transforms, for e.g. kemar on 48000hz this is 512 samples or 10.6ms

Time of the transform itself are negligible on a Ryzen 5800X 

### Checkout

```bash
git clone --recursive https://github.com/cijber/virtual-surround.git
```

## `virtual-surround`

The crate with the Logic, and fourier transforms. math.

Features:

- `rust` (default), uses the `realfft` and `rustfft` crates for FFT, this feature is in place so we can later switch out
  for other FFT implementations
- `resample` (default), compile with resampling support (by use of `libsamplerate`) this is used when the sample rate of
  the hrir is not equal to target sample rate

Totally undocumented for your own enjoyment!

## `jack-vsf`

`jack-vsf <hrir-file>`

Create a JACK based Virtual Surround filter, please do a release build, Rust in debug is a bit CPU hungry

### Build

You need to have the following installed:

* libsamplerate
* jack (or pipewire's fake jack)

```bash
cargo build -p jack-vsf --release
# and if you're feeling spicy
strip target/release/jack-vsf
```

### Running

there has to be a JACK server running already, or Fake JACK (aka pipewire)

```bash
# sample run, press enter to quit
./target/release/jack-vsf ./resources/hrir_kemar/hrir-kemar.wav
```

## `bwavfile`

Git submodule with patched `bwavfile` crate, which introduces support for PCM f32 wav files, and some other small things

## `libsamplerate-sys`

Git submodule with patched `libsamplerate-sys` crate, which allows for dynamic linking, instead of using a static build

## Additional files

This git repo contains some extra sample HRIR files, which are open to distribution

the kemar head will do fine, but you might get a better experience by finding your perfect head match in `hrir_listen`

- `resources/hrir_kemar` is licensed under MIT, uses a dummy head (1994)
- `resources/hrir_listen` is public domain, uses a bunch of random people's heads (2002 - 2003)