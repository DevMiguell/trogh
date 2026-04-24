use rand::RngExt;
use rustfft::{FftPlanner, num_complex::Complex};
use std::io::BufWriter;
use std::io::Write;
use zerocopy::{
    Immutable, IntoBytes,
    little_endian::{U16, U32},
};

#[derive(IntoBytes, Immutable)]
#[repr(u16)]
enum WaveFormatCategory {
    /// Microsoft Pulse Code Modulation (PCM) format
    Pcm = 0x0001u16.to_le(),
}

#[derive(IntoBytes, Immutable)]
#[repr(C, packed)]
struct FormatChunkCommon<FSF> {
    format_tag: WaveFormatCategory,
    channels: U16,
    samples_per_sec: U32,
    avg_bytes_per_sec: U32,
    block_align: U16,
    format_specific: FSF,
}

#[derive(IntoBytes, Immutable)]
#[repr(C, packed)]
struct FormatChunkPcm {
    bits_per_sample: U16,
}

const CHANNELS: u16 = 1;
const SAMPLES_PER_SECOND: u32 = 44100;
const BITS_PER_SAMPLE: u16 = 16;
const AVG_BYTES_PER_SECOND: u32 =
    CHANNELS as u32 * SAMPLES_PER_SECOND * (BITS_PER_SAMPLE / 8) as u32;

fn main() -> Result<(), std::io::Error> {
    let mut rng = rand::rng();
    let avg_amplitude = 8.;

    match std::env::args().nth(1).as_deref() {
        None | Some("white") => noise(|spectrum| {
            for bin in spectrum {
                *bin =
                    Complex::from_polar(avg_amplitude, rng.random::<f64>() * std::f64::consts::TAU);
            }
        })?,
        Some("pink") => noise(|spectrum| {
            let max_amplitude = avg_amplitude * f64::sqrt(22050. / 2.);
            for (hz, bin) in spectrum.iter_mut().enumerate().skip(20) {
                *bin = Complex::from_polar(
                    max_amplitude / ((hz + 1) as f64).sqrt(),
                    rng.random::<f64>() * std::f64::consts::TAU,
                );
            }
        })?,
        Some("brownian") => noise(|spectrum: &mut [Complex<f64>]| {
            let max_amplitude = avg_amplitude * (22050. / 4.);
            for (hz, bin) in spectrum.iter_mut().enumerate().skip(20) {
                *bin = Complex::from_polar(
                    max_amplitude / ((hz + 1) as f64),
                    rng.random::<f64>() * std::f64::consts::TAU,
                );
            }
        })?,
        Some(kind) => todo!("{kind} noise not suported yet"),
    }

    Ok(())
}

fn noise(mut spectrum_setup: impl FnMut(&mut [Complex<f64>])) -> Result<(), std::io::Error> {
    let duration_in_seconds = 10;
    let sample_data_len = AVG_BYTES_PER_SECOND * duration_in_seconds;
    let format = FormatChunkCommon {
        format_tag: WaveFormatCategory::Pcm,
        channels: CHANNELS.into(),
        samples_per_sec: SAMPLES_PER_SECOND.into(),
        avg_bytes_per_sec: AVG_BYTES_PER_SECOND.into(),
        block_align: (CHANNELS * BITS_PER_SAMPLE / 8).into(),
        format_specific: FormatChunkPcm {
            bits_per_sample: BITS_PER_SAMPLE.into(),
        },
    };

    let out = std::fs::File::create("audio.wav")?;
    let mut out = BufWriter::new(out);

    out.write_all(b"RIFF")?;
    out.write_all(
        &(sample_data_len + 3 * 4 + std::mem::size_of_val(&format) as u32).to_le_bytes(),
    )?;
    out.write_all(b"WAVE")?;
    write_chunk(b"fmt ", format, &mut out)?;
    // format-specific for PCM
    //
    //      WORD wBitsPerSample
    out.write_all(b"data")?;
    out.write_all(&sample_data_len.to_le_bytes())?;

    let length = SAMPLES_PER_SECOND as usize;
    let mut real_planner = FftPlanner::<f64>::new();
    let c2r = real_planner.plan_fft_inverse(length);

    let mut spectrum = [Complex::ZERO; SAMPLES_PER_SECOND as usize];
    let mut time = [Complex::ZERO; SAMPLES_PER_SECOND as usize];
    let mut scratch = Vec::new();
    scratch.resize(c2r.get_immutable_scratch_len(), Complex::ZERO);

    let mut rng = rand::rng();
    let mut dampen = -1.0;
    for interval in 0..duration_in_seconds {
        let (pos, neg) = spectrum.split_at_mut(SAMPLES_PER_SECOND as usize / 2);
        if interval == 0 {
            spectrum_setup(&mut pos[1..]);
            pos[0] = Complex::ZERO; // DC bin must be zero to avoid a constant offset in the output
        // populate conjugates
        } else {
            for (hz, bin) in pos.iter_mut().enumerate().skip(1) {
                *bin = *bin
                    * Complex::from_polar(
                        1.,
                        rng.random::<f64>() * (hz as f64 / 22050.) * std::f64::consts::FRAC_PI_2,
                    );
            }
        }
        for (bin, pos) in neg.iter_mut().skip(1).zip(pos.iter().rev()) {
            *bin = pos.conj();
        }
        neg[0] = Complex::ZERO; // Nyquist bin must be zero to avoid a constant offset in the output
        c2r.process_immutable_with_scratch(&mut spectrum[..], &mut time[..], &mut scratch[..]);

        for sample in &time {
            // let Ok(amplitude) = &i16::try_from(sample.norm().round() as i64 + i16::MIN as i64)
            // else {
            //     panic!("Amplitude out of range for i16: {}", sample);
            // };
            // assert_eq!(sample.im, 0.0);
            let amplitude = sample.re.round();
            let amplitude = amplitude + amplitude * dampen;
            let amplitude = (amplitude as i64).clamp(i16::MIN as i64, i16::MAX as i64) as i16;
            dampen = (dampen + 0.0001).min(0.0);
            out.write_all(&amplitude.to_le_bytes())?;
        }
    }

    out.flush()
}

fn write_chunk<T: IntoBytes + Immutable, W: Write>(
    fourcc: &[u8; 4],
    t: T, // what's the T? the type of the chunk data, which must implement IntoBytes and Immutable traits from zerocopy
    // but why do we need the Immutable trait? because we need to ensure that the data can be safely converted to bytes without any issues, and the Immutable trait guarantees that the data is immutable and can be safely converted to bytes
    mut out: W,
) -> Result<(), std::io::Error> {
    out.write_all(fourcc)?;
    out.write_all(&(std::mem::size_of::<T>() as u32).to_le_bytes())?;
    t.write_to_io(&mut out)?;
    Ok(())
}
