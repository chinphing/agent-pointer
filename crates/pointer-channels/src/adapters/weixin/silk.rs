use anyhow::Result;
use std::ffi::c_void;

const SILK_HEADER: &[u8] = b"#!SILK_V3";
const DEFAULT_SAMPLE_RATE: i32 = 24000;
const MAX_FRAME_SAMPLES: usize = 250 * 5;
const SILK_OK: i32 = 0;

pub fn silk_to_wav_bytes(input: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
    let pcm = decode_silk_to_pcm(input, sample_rate)?;
    Ok(pcm_to_wav(&pcm, sample_rate))
}

fn decode_silk_to_pcm(input: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
    let mut data = input;
    if data.first() == Some(&0x02) {
        data = &data[1..];
    }
    if data.len() < SILK_HEADER.len() || &data[..SILK_HEADER.len()] != SILK_HEADER {
        anyhow::bail!("weixin voice: missing SILK header");
    }
    data = &data[SILK_HEADER.len()..];

    unsafe {
        let mut dec_size: i32 = 0;
        let ret = silk_v3_sys::SKP_Silk_SDK_Get_Decoder_Size(&mut dec_size);
        if ret != SILK_OK || dec_size <= 0 {
            anyhow::bail!("silk Get_Decoder_Size failed ret={ret}");
        }
        let mut state = vec![0u8; dec_size as usize];
        let ret = silk_v3_sys::SKP_Silk_SDK_InitDecoder(state.as_mut_ptr() as *mut c_void);
        if ret != SILK_OK {
            anyhow::bail!("silk InitDecoder failed ret={ret}");
        }

        let mut dec_control = std::mem::MaybeUninit::<silk_v3_sys::SKP_SILK_SDK_DecControlStruct>::uninit();
        let dec_control = dec_control.as_mut_ptr();
        (*dec_control).API_sampleRate = sample_rate as i32;
        (*dec_control).frameSize = 0;

        let mut pcm_out = Vec::new();
        let mut offset = 0usize;
        while offset + 2 <= data.len() {
            let nlen = i16::from_le_bytes([data[offset], data[offset + 1]]) as i32;
            offset += 2;
            if nlen <= 0 {
                continue;
            }
            let nlen = nlen as usize;
            if offset + nlen > data.len() {
                break;
            }
            let frame = &data[offset..offset + nlen];
            offset += nlen;

            let mut out_samples = vec![0i16; MAX_FRAME_SAMPLES];
            let mut out_len: i16 = MAX_FRAME_SAMPLES as i16;
            let ret = silk_v3_sys::SKP_Silk_SDK_Decode(
                state.as_mut_ptr() as *mut c_void,
                dec_control,
                0,
                frame.as_ptr(),
                nlen as i32,
                out_samples.as_mut_ptr(),
                &mut out_len,
            );
            if ret != SILK_OK {
                log::warn!("silk frame decode ret={ret} nlen={nlen}");
                continue;
            }
            if out_len > 0 {
                for sample in &out_samples[..out_len as usize] {
                    pcm_out.extend_from_slice(&sample.to_le_bytes());
                }
            }
        }
        if pcm_out.is_empty() {
            anyhow::bail!("silk decode produced empty pcm");
        }
        Ok(pcm_out)
    }
}

fn pcm_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * (bits as u32 / 8);
    let block_align = channels * (bits / 8);
    let data_size = pcm.len() as u32;
    let riff_size = 36 + data_size;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

pub fn normalize_weixin_voice_bytes(
    raw: &[u8],
    encode_type: Option<u32>,
    sample_rate: Option<u32>,
) -> Result<(Vec<u8>, String)> {
    let rate = sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE as u32);
    let enc = encode_type.unwrap_or(6);
    match enc {
        6 | 4 | 5 | 8 => silk_to_wav_bytes(raw, rate).map(|b| (b, "audio/wav".into())),
        7 => Ok((raw.to_vec(), "audio/mpeg".into())),
        1 => Ok((pcm_to_wav(raw, rate), "audio/wav".into())),
        _ => {
            log::warn!("weixin voice unknown encode_type={enc}; trying silk decode");
            silk_to_wav_bytes(raw, rate).map(|b| (b, "audio/wav".into()))
        }
    }
}
