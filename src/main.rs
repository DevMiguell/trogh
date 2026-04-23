use std::io::Write;
use std::io::BufWriter;
use realfft::RealFftPlanner;
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
const AVG_BYTES_PER_SEC: u32 = SAMPLES_PER_SECOND * (BITS_PER_SAMPLE as u32 / 8) * CHANNELS as u32;

fn main() -> Result<(), std::io::Error> {
    let duration_in_seconds = 10;
    let sample_data_len = AVG_BYTES_PER_SEC * duration_in_seconds;
    let format = FormatChunkCommon {
        format_tag: WaveFormatCategory::Pcm,
        channels: CHANNELS.into(),
        samples_per_sec: SAMPLES_PER_SECOND.into(),
        avg_bytes_per_sec: AVG_BYTES_PER_SEC.into(),
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
    let mut real_planner = RealFftPlanner::<f64>::new();

    let r2c = real_planner.plan_fft_inverse(length);
    let mut spectrum = r2c.make_input_vec();
    // println!("spectrum length: {}", spectrum.len());
    spectrum[440] = (1000.).into();
    let mut time = r2c.make_output_vec();

    r2c.process(&mut spectrum, &mut time).unwrap();

    for _interval in 0..duration_in_seconds {
        for sample in &time {
            let amplitude = &i16::try_from(sample.round() as i64).unwrap();
            // println!("{amplitude}");
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
