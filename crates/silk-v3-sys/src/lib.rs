//! Vendored SILK v3 decoder FFI (hand-written bindings; no libclang at build time).

#![allow(dead_code, non_camel_case_types, non_snake_case, non_upper_case_globals)]

use std::os::raw::{c_int, c_short, c_uchar, c_void};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SKP_SILK_SDK_DecControlStruct {
    pub API_sampleRate: c_int,
    pub frameSize: c_int,
    pub framesPerPacket: c_int,
    pub moreInternalDecoderFrames: c_int,
    pub inBandFECOffset: c_int,
}

extern "C" {
    pub fn SKP_Silk_SDK_Get_Decoder_Size(decSizeBytes: *mut c_int) -> c_int;
    pub fn SKP_Silk_SDK_InitDecoder(decState: *mut c_void) -> c_int;
    pub fn SKP_Silk_SDK_Decode(
        decState: *mut c_void,
        decControl: *mut SKP_SILK_SDK_DecControlStruct,
        lostFlag: c_int,
        inData: *const c_uchar,
        nBytesIn: c_int,
        samplesOut: *mut c_short,
        nSamplesOut: *mut c_short,
    ) -> c_int;
}
