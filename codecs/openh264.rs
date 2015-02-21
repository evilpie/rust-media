// Copyright 2015 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![allow(missing_copy_implementations)]

use codecs::h264;
use pixelformat::PixelFormat;
use timing::Timestamp;
use videodecoder;

use libc::{c_uchar, c_int, c_long, c_uint};
use std::ptr;
use std::slice;
use std::{u8, u32};
use std::default::Default;
use std::mem;

pub struct OpenH264Codec {
    decoder: *mut ffi::ISVCDecoder,
}

impl OpenH264Codec {
    pub fn init(headers: &videodecoder::VideoHeaders) -> Result<OpenH264Codec,c_long> {
        unsafe {
            let mut decoder: *mut ffi::ISVCDecoder = mem::zeroed();
            let err = ffi::WelsCreateDecoder(&mut decoder);
            if err != 0 {
                return Err(err)
            }

            let mut decParam: ffi::SDecodingParam = Default::default();
            decParam.eOutputColorFormat = ffi::videoFormatI420;
            decParam.uiTargetDqLayer = u8::MAX;
            decParam.eEcActiveIdc = ffi::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE;
            decParam.sVideoProperty.size = 8;
            decParam.sVideoProperty.eVideoBsType = ffi::VIDEO_BITSTREAM_AVC; //  ffi::VIDEO_BITSTREAM_DEFAULT;

            let result = ((**decoder).Initialize)(decoder, &decParam);
            if result != 0 {
                return Err(result);
            }

            let avcc = h264::create_avcc_chunk(headers);

            let mut dst: *mut c_uchar = mem::zeroed();
            let mut decoded: ffi::SBufferInfo = Default::default();

            let state = ((**decoder).DecodeFrame2)(decoder, avcc.as_ptr(), avcc.len() as c_int,
                                                   &mut dst, &mut decoded);

            println!("initial state {:?}", state);

            Ok(OpenH264Codec {
                decoder: decoder,
            })
        }
    }

    pub fn decode(&self, data: &[u8], deadline: c_long) -> Option<OpenH264Image> {
        assert!(data.len() <= (u32::MAX as usize));

        let mut stride: c_int = 0;
        let mut width: c_int = 0;
        let mut height: c_int = 0;

        // this should be [3]
        let mut dst: *mut c_uchar = unsafe {
            mem::zeroed()
        };

        // println!("data: {:?}", data);

        let state = unsafe {
            ((**self.decoder).DecodeFrame)(self.decoder,
                                           data.as_ptr(), data.len() as c_int,
                                           &mut dst, &mut stride,
                                           &mut width, &mut height)
        };

        println!("state: {:?}", state);
        // println!("stride: {:?}", stride);

        Some(OpenH264Image {
            data: dst,
            stride: stride,
            width: width,
            height: height
        })
    }
}

pub struct OpenH264Image {
    data: *mut c_uchar,
    stride: c_int,
    width: c_int,
    height: c_int
}

impl Drop for OpenH264Image {
    fn drop(&mut self) {
        unsafe {
            // XXX Figure this out!!!
            // ffi::vpx_img_free(self.image)
        }
    }
}

impl OpenH264Image {
    pub fn width(&self) -> c_uint {
        self.width as c_uint
    }

    pub fn height(&self) -> c_uint {
        self.height as c_uint
    }

    pub fn bit_depth(&self) -> c_uint {
        0 // xxx fixme
    }

    pub fn stride(&self, index: c_uint) -> c_int {
        assert!(index < 4);
        // XXX fixme
        self.stride
        // unsafe {
        //     (*self.image).stride[index as c_int]
        // }
    }

    pub fn plane<'a>(&'a self, index: c_uint) -> &'a [u8] {
        assert!(index < 4);
        // unsafe {
        //     let len = (self.stride(index) as c_uint) * (*self.image).h;
        //     slice::from_raw_mut_buf(&(*self.image).planes[index as c_int], len as usize)
        // }
        unsafe {
            let len = self.stride * self.height;
            slice::from_raw_mut_buf(&self.data, len as usize)
        }
    }

    pub fn bps(&self) -> c_int {
        // XXX fixme
        0
    }
}

// Implementation of the abstract `VideoDecoder` interface

struct VideoDecoderImpl {
    codec: OpenH264Codec,
}

impl VideoDecoderImpl {
    fn new(headers: &videodecoder::VideoHeaders, _: i32, _: i32)
           -> Result<Box<videodecoder::VideoDecoder + 'static>,()> {
        match OpenH264Codec::init(headers) {
            Ok(codec) => {
                Ok(Box::new(VideoDecoderImpl {
                    codec: codec,
                }) as Box<videodecoder::VideoDecoder>)
            }
            Err(_) => Err(()),
        }
    }
}

impl videodecoder::VideoDecoder for VideoDecoderImpl {
    fn decode_frame(&self, data: &[u8], presentation_time: &Timestamp)
                    -> Result<Box<videodecoder::DecodedVideoFrame + 'static>,()> {
        let image = match self.codec.decode(data, 0) {
            None => return Err(()),
            Some(image) => image,
        };
        Ok(Box::new(DecodedVideoFrameImpl {
            image: image,
            presentation_time: *presentation_time,
        }) as Box<videodecoder::DecodedVideoFrame>)
    }
}

struct DecodedVideoFrameImpl {
    image: OpenH264Image,
    presentation_time: Timestamp,
}

impl videodecoder::DecodedVideoFrame for DecodedVideoFrameImpl {
    fn width(&self) -> c_uint {
        self.image.width()
    }

    fn height(&self) -> c_uint {
        self.image.height()
    }

    fn stride(&self, index: usize) -> c_int {
        self.image.stride(index as u32)
    }

    fn pixel_format<'a>(&'a self) -> PixelFormat<'a> {
        PixelFormat::I420
    }

    fn presentation_time(&self) -> Timestamp {
        self.presentation_time
    }

    fn lock<'a>(&'a self) -> Box<videodecoder::DecodedVideoFrameLockGuard + 'a> {
        Box::new(DecodedVideoFrameLockGuardImpl {
            image: &self.image,
        }) as Box<videodecoder::DecodedVideoFrameLockGuard + 'a>
    }
}

struct DecodedVideoFrameLockGuardImpl<'a> {
    image: &'a OpenH264Image,
}

impl<'a> videodecoder::DecodedVideoFrameLockGuard for DecodedVideoFrameLockGuardImpl<'a> {
    fn pixels<'b>(&'b self, plane_index: usize) -> &'b [u8] {
        self.image.plane(plane_index as u32)
    }
}

pub const VIDEO_DECODER: videodecoder::RegisteredVideoDecoder =
    videodecoder::RegisteredVideoDecoder {
        id: [ b'a', b'v', b'c', b' ' ],
        constructor: VideoDecoderImpl::new,
    };


pub mod ffi {

use libc::{c_longlong, c_uint, c_uchar, c_int, c_void};
use std::mem;
use std::default::Default;

/* automatically generated by rust-bindgen */


pub type _bool = ::libc::c_uchar;
pub type Enum_Unnamed1 = ::libc::c_uint;
pub const videoFormatRGB: ::libc::c_uint = 1;
pub const videoFormatRGBA: ::libc::c_uint = 2;
pub const videoFormatRGB555: ::libc::c_uint = 3;
pub const videoFormatRGB565: ::libc::c_uint = 4;
pub const videoFormatBGR: ::libc::c_uint = 5;
pub const videoFormatBGRA: ::libc::c_uint = 6;
pub const videoFormatABGR: ::libc::c_uint = 7;
pub const videoFormatARGB: ::libc::c_uint = 8;
pub const videoFormatYUY2: ::libc::c_uint = 20;
pub const videoFormatYVYU: ::libc::c_uint = 21;
pub const videoFormatUYVY: ::libc::c_uint = 22;
pub const videoFormatI420: ::libc::c_uint = 23;
pub const videoFormatYV12: ::libc::c_uint = 24;
pub const videoFormatInternal: ::libc::c_uint = 25;
pub const videoFormatNV12: ::libc::c_uint = 26;
pub const videoFormatVFlip: ::libc::c_uint = -2147483648;
pub type EVideoFormatType = Enum_Unnamed1;
pub type Enum_Unnamed2 = ::libc::c_uint;
pub const videoFrameTypeInvalid: ::libc::c_uint = 0;
pub const videoFrameTypeIDR: ::libc::c_uint = 1;
pub const videoFrameTypeI: ::libc::c_uint = 2;
pub const videoFrameTypeP: ::libc::c_uint = 3;
pub const videoFrameTypeSkip: ::libc::c_uint = 4;
pub const videoFrameTypeIPMixed: ::libc::c_uint = 5;
pub type EVideoFrameType = Enum_Unnamed2;
pub type Enum_Unnamed3 = ::libc::c_uint;
pub const cmResultSuccess: ::libc::c_uint = 0;
pub const cmInitParaError: ::libc::c_uint = 1;
pub const cmUnkonwReason: ::libc::c_uint = 2;
pub const cmMallocMemeError: ::libc::c_uint = 3;
pub const cmInitExpected: ::libc::c_uint = 4;
pub const cmUnsupportedData: ::libc::c_uint = 5;
pub type CM_RETURN = Enum_Unnamed3;
pub type Enum_ENalUnitType = ::libc::c_uint;
pub const NAL_UNKNOWN: ::libc::c_uint = 0;
pub const NAL_SLICE: ::libc::c_uint = 1;
pub const NAL_SLICE_DPA: ::libc::c_uint = 2;
pub const NAL_SLICE_DPB: ::libc::c_uint = 3;
pub const NAL_SLICE_DPC: ::libc::c_uint = 4;
pub const NAL_SLICE_IDR: ::libc::c_uint = 5;
pub const NAL_SEI: ::libc::c_uint = 6;
pub const NAL_SPS: ::libc::c_uint = 7;
pub const NAL_PPS: ::libc::c_uint = 8;
pub type Enum_ENalPriority = ::libc::c_uint;
pub const NAL_PRIORITY_DISPOSABLE: ::libc::c_uint = 0;
pub const NAL_PRIORITY_LOW: ::libc::c_uint = 1;
pub const NAL_PRIORITY_HIGH: ::libc::c_uint = 2;
pub const NAL_PRIORITY_HIGHEST: ::libc::c_uint = 3;
pub type ERR_TOOL = ::libc::c_ushort;
pub type Enum_Unnamed4 = ::libc::c_uint;
pub const ET_NONE: ::libc::c_uint = 0;
pub const ET_IP_SCALE: ::libc::c_uint = 1;
pub const ET_FMO: ::libc::c_uint = 2;
pub const ET_IR_R1: ::libc::c_uint = 4;
pub const ET_IR_R2: ::libc::c_uint = 8;
pub const ET_IR_R3: ::libc::c_uint = 16;
pub const ET_FEC_HALF: ::libc::c_uint = 32;
pub const ET_FEC_FULL: ::libc::c_uint = 64;
pub const ET_RFS: ::libc::c_uint = 128;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_SliceInformation {
    pub pBufferOfSlices: *mut ::libc::c_uchar,
    pub iCodedSliceCount: ::libc::c_int,
    pub pLengthOfSlices: *mut ::libc::c_uint,
    pub iFecType: ::libc::c_int,
    pub uiSliceIdx: ::libc::c_uchar,
    pub uiSliceCount: ::libc::c_uchar,
    pub iFrameIndex: ::libc::c_char,
    pub uiNalRefIdc: ::libc::c_uchar,
    pub uiNalType: ::libc::c_uchar,
    pub uiContainingFinalNal: ::libc::c_uchar,
}
impl ::std::default::Default for Struct_SliceInformation {
    fn default() -> Struct_SliceInformation {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SliceInfo = Struct_SliceInformation;
pub type PSliceInfo = *mut Struct_SliceInformation;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed5 {
    pub iWidth: ::libc::c_int,
    pub iHeight: ::libc::c_int,
    pub iThresholdOfInitRate: ::libc::c_int,
    pub iThresholdOfMaxRate: ::libc::c_int,
    pub iThresholdOfMinRate: ::libc::c_int,
    pub iMinThresholdFrameRate: ::libc::c_int,
    pub iSkipFrameRate: ::libc::c_int,
    pub iSkipFrameStep: ::libc::c_int,
}
impl ::std::default::Default for Struct_Unnamed5 {
    fn default() -> Struct_Unnamed5 { unsafe { ::std::mem::zeroed() } }
}
pub type SRateThresholds = Struct_Unnamed5;
pub type PRateThresholds = *mut Struct_Unnamed5;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagSysMemBuffer {
    pub iWidth: ::libc::c_int,
    pub iHeight: ::libc::c_int,
    pub iFormat: ::libc::c_int,
    pub iStride: [::libc::c_int; 2],
}
impl ::std::default::Default for Struct_TagSysMemBuffer {
    fn default() -> Struct_TagSysMemBuffer { unsafe { ::std::mem::zeroed() } }
}
pub type SSysMEMBuffer = Struct_TagSysMemBuffer;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagBufferInfo {
    pub iBufferStatus: ::libc::c_int,
    pub uiInBsTimeStamp: ::libc::c_ulonglong,
    pub uiOutYuvTimeStamp: ::libc::c_ulonglong,
    pub UsrData: Union_Unnamed6,
}
impl ::std::default::Default for Struct_TagBufferInfo {
    fn default() -> Struct_TagBufferInfo { unsafe { ::std::mem::zeroed() } }
}
#[repr(C)]
#[derive(Copy)]
pub struct Union_Unnamed6 {
    pub _bindgen_data_: [u32; 5],
}
impl Union_Unnamed6 {
    pub unsafe fn sSystemBuffer(&mut self) -> *mut SSysMEMBuffer {
        ::std::mem::transmute(&self._bindgen_data_)
    }
}
impl ::std::default::Default for Union_Unnamed6 {
    fn default() -> Union_Unnamed6 { unsafe { ::std::mem::zeroed() } }
}
pub type SBufferInfo = Struct_TagBufferInfo;
#[repr(C)]
#[derive(Copy)]
pub struct Struct__tagVersion {
    pub uMajor: ::libc::c_uint,
    pub uMinor: ::libc::c_uint,
    pub uRevision: ::libc::c_uint,
    pub uReserved: ::libc::c_uint,
}
impl ::std::default::Default for Struct__tagVersion {
    fn default() -> Struct__tagVersion { unsafe { ::std::mem::zeroed() } }
}
pub type OpenH264Version = Struct__tagVersion;
pub type Enum_Unnamed7 = ::libc::c_uint;
pub const dsErrorFree: ::libc::c_uint = 0;
pub const dsFramePending: ::libc::c_uint = 1;
pub const dsRefLost: ::libc::c_uint = 2;
pub const dsBitstreamError: ::libc::c_uint = 4;
pub const dsDepLayerLost: ::libc::c_uint = 8;
pub const dsNoParamSets: ::libc::c_uint = 16;
pub const dsDataErrorConcealed: ::libc::c_uint = 32;
pub const dsInvalidArgument: ::libc::c_uint = 4096;
pub const dsInitialOptExpected: ::libc::c_uint = 8192;
pub const dsOutOfMemory: ::libc::c_uint = 16384;
pub const dsDstBufNeedExpan: ::libc::c_uint = 32768;
pub type DECODING_STATE = Enum_Unnamed7;
pub type Enum_Unnamed8 = ::libc::c_uint;
pub const ENCODER_OPTION_DATAFORMAT: ::libc::c_uint = 0;
pub const ENCODER_OPTION_IDR_INTERVAL: ::libc::c_uint = 1;
pub const ENCODER_OPTION_SVC_ENCODE_PARAM_BASE: ::libc::c_uint = 2;
pub const ENCODER_OPTION_SVC_ENCODE_PARAM_EXT: ::libc::c_uint = 3;
pub const ENCODER_OPTION_FRAME_RATE: ::libc::c_uint = 4;
pub const ENCODER_OPTION_BITRATE: ::libc::c_uint = 5;
pub const ENCODER_OPTION_MAX_BITRATE: ::libc::c_uint = 6;
pub const ENCODER_OPTION_INTER_SPATIAL_PRED: ::libc::c_uint = 7;
pub const ENCODER_OPTION_RC_MODE: ::libc::c_uint = 8;
pub const ENCODER_PADDING_PADDING: ::libc::c_uint = 9;
pub const ENCODER_OPTION_PROFILE: ::libc::c_uint = 10;
pub const ENCODER_OPTION_LEVEL: ::libc::c_uint = 11;
pub const ENCODER_OPTION_NUMBER_REF: ::libc::c_uint = 12;
pub const ENCODER_OPTION_DELIVERY_STATUS: ::libc::c_uint = 13;
pub const ENCODER_LTR_RECOVERY_REQUEST: ::libc::c_uint = 14;
pub const ENCODER_LTR_MARKING_FEEDBACK: ::libc::c_uint = 15;
pub const ENCODER_LTR_MARKING_PERIOD: ::libc::c_uint = 16;
pub const ENCODER_OPTION_LTR: ::libc::c_uint = 17;
pub const ENCODER_OPTION_COMPLEXITY: ::libc::c_uint = 18;
pub const ENCODER_OPTION_ENABLE_SSEI: ::libc::c_uint = 19;
pub const ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING: ::libc::c_uint = 20;
pub const ENCODER_OPTION_ENABLE_SPS_PPS_ID_ADDITION: ::libc::c_uint = 21;
pub const ENCODER_OPTION_CURRENT_PATH: ::libc::c_uint = 22;
pub const ENCODER_OPTION_DUMP_FILE: ::libc::c_uint = 23;
pub const ENCODER_OPTION_TRACE_LEVEL: ::libc::c_uint = 24;
pub const ENCODER_OPTION_TRACE_CALLBACK: ::libc::c_uint = 25;
pub const ENCODER_OPTION_TRACE_CALLBACK_CONTEXT: ::libc::c_uint = 26;
pub const ENCODER_OPTION_GET_STATISTICS: ::libc::c_uint = 27;
pub const ENCODER_OPTION_STATISTICS_LOG_INTERVAL: ::libc::c_uint = 28;
pub const ENCODER_OPTION_IS_LOSSLESS_LINK: ::libc::c_uint = 29;
pub const ENCODER_OPTION_BITS_VARY_PERCENTAGE: ::libc::c_uint = 30;
pub type ENCODER_OPTION = Enum_Unnamed8;
pub type Enum_Unnamed9 = ::libc::c_uint;
pub const DECODER_OPTION_DATAFORMAT: ::libc::c_uint = 0;
pub const DECODER_OPTION_END_OF_STREAM: ::libc::c_uint = 1;
pub const DECODER_OPTION_VCL_NAL: ::libc::c_uint = 2;
pub const DECODER_OPTION_TEMPORAL_ID: ::libc::c_uint = 3;
pub const DECODER_OPTION_FRAME_NUM: ::libc::c_uint = 4;
pub const DECODER_OPTION_IDR_PIC_ID: ::libc::c_uint = 5;
pub const DECODER_OPTION_LTR_MARKING_FLAG: ::libc::c_uint = 6;
pub const DECODER_OPTION_LTR_MARKED_FRAME_NUM: ::libc::c_uint = 7;
pub const DECODER_OPTION_ERROR_CON_IDC: ::libc::c_uint = 8;
pub const DECODER_OPTION_TRACE_LEVEL: ::libc::c_uint = 9;
pub const DECODER_OPTION_TRACE_CALLBACK: ::libc::c_uint = 10;
pub const DECODER_OPTION_TRACE_CALLBACK_CONTEXT: ::libc::c_uint = 11;
pub const DECODER_OPTION_GET_STATISTICS: ::libc::c_uint = 12;
pub type DECODER_OPTION = Enum_Unnamed9;
pub type Enum_Unnamed10 = ::libc::c_uint;
pub const ERROR_CON_DISABLE: ::libc::c_uint = 0;
pub const ERROR_CON_FRAME_COPY: ::libc::c_uint = 1;
pub const ERROR_CON_SLICE_COPY: ::libc::c_uint = 2;
pub const ERROR_CON_FRAME_COPY_CROSS_IDR: ::libc::c_uint = 3;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR: ::libc::c_uint = 4;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE: ::libc::c_uint =
    5;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR: ::libc::c_uint = 6;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE: ::libc::c_uint
          =
    7;
pub type ERROR_CON_IDC = Enum_Unnamed10;
pub type Enum_Unnamed11 = ::libc::c_uint;
pub const FEEDBACK_NON_VCL_NAL: ::libc::c_uint = 0;
pub const FEEDBACK_VCL_NAL: ::libc::c_uint = 1;
pub const FEEDBACK_UNKNOWN_NAL: ::libc::c_uint = 2;
pub type FEEDBACK_VCL_NAL_IN_AU = Enum_Unnamed11;
pub type Enum_Unnamed12 = ::libc::c_uint;
pub const NON_VIDEO_CODING_LAYER: ::libc::c_uint = 0;
pub const VIDEO_CODING_LAYER: ::libc::c_uint = 1;
pub type LAYER_TYPE = Enum_Unnamed12;
pub type Enum_Unnamed13 = ::libc::c_uint;
pub const SPATIAL_LAYER_0: ::libc::c_uint = 0;
pub const SPATIAL_LAYER_1: ::libc::c_uint = 1;
pub const SPATIAL_LAYER_2: ::libc::c_uint = 2;
pub const SPATIAL_LAYER_3: ::libc::c_uint = 3;
pub const SPATIAL_LAYER_ALL: ::libc::c_uint = 4;
pub type LAYER_NUM = Enum_Unnamed13;
pub type Enum_Unnamed14 = ::libc::c_uint;
pub const VIDEO_BITSTREAM_AVC: ::libc::c_uint = 0;
pub const VIDEO_BITSTREAM_SVC: ::libc::c_uint = 1;
pub const VIDEO_BITSTREAM_DEFAULT: ::libc::c_uint = 1;
pub type VIDEO_BITSTREAM_TYPE = Enum_Unnamed14;
pub type Enum_Unnamed15 = ::libc::c_uint;
pub const NO_RECOVERY_REQUSET: ::libc::c_uint = 0;
pub const LTR_RECOVERY_REQUEST: ::libc::c_uint = 1;
pub const IDR_RECOVERY_REQUEST: ::libc::c_uint = 2;
pub const NO_LTR_MARKING_FEEDBACK: ::libc::c_uint = 3;
pub const LTR_MARKING_SUCCESS: ::libc::c_uint = 4;
pub const LTR_MARKING_FAILED: ::libc::c_uint = 5;
pub type KEY_FRAME_REQUEST_TYPE = Enum_Unnamed15;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed16 {
    pub uiFeedbackType: ::libc::c_uint,
    pub uiIDRPicId: ::libc::c_uint,
    pub iLastCorrectFrameNum: ::libc::c_int,
    pub iCurrentFrameNum: ::libc::c_int,
}
impl ::std::default::Default for Struct_Unnamed16 {
    fn default() -> Struct_Unnamed16 { unsafe { ::std::mem::zeroed() } }
}
pub type SLTRRecoverRequest = Struct_Unnamed16;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed17 {
    pub uiFeedbackType: ::libc::c_uint,
    pub uiIDRPicId: ::libc::c_uint,
    pub iLTRFrameNum: ::libc::c_int,
}
impl ::std::default::Default for Struct_Unnamed17 {
    fn default() -> Struct_Unnamed17 { unsafe { ::std::mem::zeroed() } }
}
pub type SLTRMarkingFeedback = Struct_Unnamed17;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed18 {
    pub bEnableLongTermReference: _bool,
    pub iLTRRefNum: ::libc::c_int,
}
impl ::std::default::Default for Struct_Unnamed18 {
    fn default() -> Struct_Unnamed18 { unsafe { ::std::mem::zeroed() } }
}
pub type SLTRConfig = Struct_Unnamed18;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed19 {
    pub uiSliceMbNum: [::libc::c_uint; 35],
    pub uiSliceNum: ::libc::c_uint,
    pub uiSliceSizeConstraint: ::libc::c_uint,
}
impl ::std::default::Default for Struct_Unnamed19 {
    fn default() -> Struct_Unnamed19 { unsafe { ::std::mem::zeroed() } }
}
pub type SSliceArgument = Struct_Unnamed19;
pub type Enum_Unnamed20 = ::libc::c_uint;
pub const SM_SINGLE_SLICE: ::libc::c_uint = 0;
pub const SM_FIXEDSLCNUM_SLICE: ::libc::c_uint = 1;
pub const SM_RASTER_SLICE: ::libc::c_uint = 2;
pub const SM_ROWMB_SLICE: ::libc::c_uint = 3;
pub const SM_DYN_SLICE: ::libc::c_uint = 4;
pub const SM_AUTO_SLICE: ::libc::c_uint = 5;
pub const SM_RESERVED: ::libc::c_uint = 6;
pub type SliceModeEnum = Enum_Unnamed20;
pub type Enum_Unnamed21 = ::libc::c_int;
pub const RC_QUALITY_MODE: ::libc::c_int = 0;
pub const RC_BITRATE_MODE: ::libc::c_int = 1;
pub const RC_BUFFERBASED_MODE: ::libc::c_int = 2;
pub const RC_TIMESTAMP_MODE: ::libc::c_int = 3;
pub const RC_OFF_MODE: ::libc::c_int = -1;
pub type RC_MODES = Enum_Unnamed21;
pub type Enum_Unnamed22 = ::libc::c_uint;
pub const PRO_UNKNOWN: ::libc::c_uint = 0;
pub const PRO_BASELINE: ::libc::c_uint = 66;
pub const PRO_MAIN: ::libc::c_uint = 77;
pub const PRO_EXTENDED: ::libc::c_uint = 88;
pub const PRO_HIGH: ::libc::c_uint = 100;
pub const PRO_HIGH10: ::libc::c_uint = 110;
pub const PRO_HIGH422: ::libc::c_uint = 122;
pub const PRO_HIGH444: ::libc::c_uint = 144;
pub const PRO_CAVLC444: ::libc::c_uint = 244;
pub const PRO_SCALABLE_BASELINE: ::libc::c_uint = 83;
pub const PRO_SCALABLE_HIGH: ::libc::c_uint = 86;
pub type EProfileIdc = Enum_Unnamed22;
pub type Enum_Unnamed23 = ::libc::c_uint;
pub const LEVEL_UNKNOWN: ::libc::c_uint = 0;
pub const LEVEL_1_0: ::libc::c_uint = 1;
pub const LEVEL_1_B: ::libc::c_uint = 2;
pub const LEVEL_1_1: ::libc::c_uint = 3;
pub const LEVEL_1_2: ::libc::c_uint = 4;
pub const LEVEL_1_3: ::libc::c_uint = 5;
pub const LEVEL_2_0: ::libc::c_uint = 6;
pub const LEVEL_2_1: ::libc::c_uint = 7;
pub const LEVEL_2_2: ::libc::c_uint = 8;
pub const LEVEL_3_0: ::libc::c_uint = 9;
pub const LEVEL_3_1: ::libc::c_uint = 10;
pub const LEVEL_3_2: ::libc::c_uint = 11;
pub const LEVEL_4_0: ::libc::c_uint = 12;
pub const LEVEL_4_1: ::libc::c_uint = 13;
pub const LEVEL_4_2: ::libc::c_uint = 14;
pub const LEVEL_5_0: ::libc::c_uint = 15;
pub const LEVEL_5_1: ::libc::c_uint = 16;
pub const LEVEL_5_2: ::libc::c_uint = 17;
pub type ELevelIdc = Enum_Unnamed23;
pub type Enum_Unnamed24 = ::libc::c_uint;
pub const WELS_LOG_QUIET: ::libc::c_uint = 0;
pub const WELS_LOG_ERROR: ::libc::c_uint = 1;
pub const WELS_LOG_WARNING: ::libc::c_uint = 2;
pub const WELS_LOG_INFO: ::libc::c_uint = 4;
pub const WELS_LOG_DEBUG: ::libc::c_uint = 8;
pub const WELS_LOG_DETAIL: ::libc::c_uint = 16;
pub const WELS_LOG_RESV: ::libc::c_uint = 32;
pub const WELS_LOG_LEVEL_COUNT: ::libc::c_uint = 6;
pub const WELS_LOG_DEFAULT: ::libc::c_uint = 2;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed25 {
    pub uiSliceMode: SliceModeEnum,
    pub sSliceArgument: SSliceArgument,
}
impl ::std::default::Default for Struct_Unnamed25 {
    fn default() -> Struct_Unnamed25 { unsafe { ::std::mem::zeroed() } }
}
pub type SSliceConfig = Struct_Unnamed25;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed26 {
    pub iVideoWidth: ::libc::c_int,
    pub iVideoHeight: ::libc::c_int,
    pub fFrameRate: ::libc::c_float,
    pub iSpatialBitrate: ::libc::c_int,
    pub iMaxSpatialBitrate: ::libc::c_int,
    pub uiProfileIdc: EProfileIdc,
    pub uiLevelIdc: ELevelIdc,
    pub iDLayerQp: ::libc::c_int,
    pub sSliceCfg: SSliceConfig,
}
impl ::std::default::Default for Struct_Unnamed26 {
    fn default() -> Struct_Unnamed26 { unsafe { ::std::mem::zeroed() } }
}
pub type SSpatialLayerConfig = Struct_Unnamed26;
pub type Enum_Unnamed27 = ::libc::c_uint;
pub const CAMERA_VIDEO_REAL_TIME: ::libc::c_uint = 0;
pub const SCREEN_CONTENT_REAL_TIME: ::libc::c_uint = 1;
pub const CAMERA_VIDEO_NON_REAL_TIME: ::libc::c_uint = 2;
pub type EUsageType = Enum_Unnamed27;
pub type Enum_Unnamed28 = ::libc::c_uint;
pub const LOW_COMPLEXITY: ::libc::c_uint = 0;
pub const MEDIUM_COMPLEXITY: ::libc::c_uint = 1;
pub const HIGH_COMPLEXITY: ::libc::c_uint = 2;
pub type ECOMPLEXITY_MODE = Enum_Unnamed28;
pub type Enum_Unnamed29 = ::libc::c_uint;
pub const CONSTANT_ID: ::libc::c_uint = 0;
pub const INCREASING_ID: ::libc::c_uint = 1;
pub const SPS_LISTING: ::libc::c_uint = 2;
pub const SPS_LISTING_AND_PPS_INCREASING: ::libc::c_uint = 3;
pub const SPS_PPS_LISTING: ::libc::c_uint = 6;
pub type EParameterSetStrategy = Enum_Unnamed29;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagEncParamBase {
    pub iUsageType: EUsageType,
    pub iPicWidth: ::libc::c_int,
    pub iPicHeight: ::libc::c_int,
    pub iTargetBitrate: ::libc::c_int,
    pub iRCMode: RC_MODES,
    pub fMaxFrameRate: ::libc::c_float,
}
impl ::std::default::Default for Struct_TagEncParamBase {
    fn default() -> Struct_TagEncParamBase { unsafe { ::std::mem::zeroed() } }
}
pub type SEncParamBase = Struct_TagEncParamBase;
pub type PEncParamBase = *mut Struct_TagEncParamBase;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagEncParamExt {
    pub iUsageType: EUsageType,
    pub iPicWidth: ::libc::c_int,
    pub iPicHeight: ::libc::c_int,
    pub iTargetBitrate: ::libc::c_int,
    pub iRCMode: RC_MODES,
    pub fMaxFrameRate: ::libc::c_float,
    pub iTemporalLayerNum: ::libc::c_int,
    pub iSpatialLayerNum: ::libc::c_int,
    pub sSpatialLayers: [SSpatialLayerConfig; 4],
    pub iComplexityMode: ECOMPLEXITY_MODE,
    pub uiIntraPeriod: ::libc::c_uint,
    pub iNumRefFrame: ::libc::c_int,
    pub eSpsPpsIdStrategy: EParameterSetStrategy,
    pub bPrefixNalAddingCtrl: _bool,
    pub bEnableSSEI: _bool,
    pub bSimulcastAVC: _bool,
    pub iPaddingFlag: ::libc::c_int,
    pub iEntropyCodingModeFlag: ::libc::c_int,
    pub bEnableFrameSkip: _bool,
    pub iMaxBitrate: ::libc::c_int,
    pub iMaxQp: ::libc::c_int,
    pub iMinQp: ::libc::c_int,
    pub uiMaxNalSize: ::libc::c_uint,
    pub bEnableLongTermReference: _bool,
    pub iLTRRefNum: ::libc::c_int,
    pub iLtrMarkPeriod: ::libc::c_uint,
    pub iMultipleThreadIdc: ::libc::c_ushort,
    pub iLoopFilterDisableIdc: ::libc::c_int,
    pub iLoopFilterAlphaC0Offset: ::libc::c_int,
    pub iLoopFilterBetaOffset: ::libc::c_int,
    pub bEnableDenoise: _bool,
    pub bEnableBackgroundDetection: _bool,
    pub bEnableAdaptiveQuant: _bool,
    pub bEnableFrameCroppingFlag: _bool,
    pub bEnableSceneChangeDetect: _bool,
    pub bIsLosslessLink: _bool,
}
impl ::std::default::Default for Struct_TagEncParamExt {
    fn default() -> Struct_TagEncParamExt { unsafe { ::std::mem::zeroed() } }
}
pub type SEncParamExt = Struct_TagEncParamExt;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed30 {
    pub size: ::libc::c_uint,
    pub eVideoBsType: VIDEO_BITSTREAM_TYPE,
}
impl ::std::default::Default for Struct_Unnamed30 {
    fn default() -> Struct_Unnamed30 { unsafe { ::std::mem::zeroed() } }
}
pub type SVideoProperty = Struct_Unnamed30;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagSVCDecodingParam {
    pub pFileNameRestructed: *mut ::libc::c_char,
    pub eOutputColorFormat: EVideoFormatType,
    pub uiCpuLoad: ::libc::c_uint,
    pub uiTargetDqLayer: ::libc::c_uchar,
    pub eEcActiveIdc: ERROR_CON_IDC,
    pub bParseOnly: _bool,
    pub sVideoProperty: SVideoProperty,
}
impl ::std::default::Default for Struct_TagSVCDecodingParam {
    fn default() -> Struct_TagSVCDecodingParam {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SDecodingParam = Struct_TagSVCDecodingParam;
pub type PDecodingParam = *mut Struct_TagSVCDecodingParam;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed31 {
    pub uiTemporalId: ::libc::c_uchar,
    pub uiSpatialId: ::libc::c_uchar,
    pub uiQualityId: ::libc::c_uchar,
    pub uiLayerType: ::libc::c_uchar,
    pub iNalCount: ::libc::c_int,
    pub pNalLengthInByte: *mut ::libc::c_int,
    pub pBsBuf: *mut ::libc::c_uchar,
}
impl ::std::default::Default for Struct_Unnamed31 {
    fn default() -> Struct_Unnamed31 { unsafe { ::std::mem::zeroed() } }
}
pub type SLayerBSInfo = Struct_Unnamed31;
pub type PLayerBSInfo = *mut Struct_Unnamed31;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Unnamed32 {
    pub iTemporalId: ::libc::c_int,
    pub iSubSeqId: ::libc::c_int,
    pub iLayerNum: ::libc::c_int,
    pub sLayerInfo: [SLayerBSInfo; 128],
    pub eFrameType: EVideoFrameType,
    pub iFrameSizeInBytes: ::libc::c_int,
    pub uiTimeStamp: ::libc::c_longlong,
}
impl ::std::default::Default for Struct_Unnamed32 {
    fn default() -> Struct_Unnamed32 { unsafe { ::std::mem::zeroed() } }
}
pub type SFrameBSInfo = Struct_Unnamed32;
pub type PFrameBSInfo = *mut Struct_Unnamed32;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_Source_Picture_s {
    pub iColorFormat: ::libc::c_int,
    pub iStride: [::libc::c_int; 4],
    pub pData: [*mut ::libc::c_uchar; 4],
    pub iPicWidth: ::libc::c_int,
    pub iPicHeight: ::libc::c_int,
    pub uiTimeStamp: ::libc::c_longlong,
}
impl ::std::default::Default for Struct_Source_Picture_s {
    fn default() -> Struct_Source_Picture_s {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SSourcePicture = Struct_Source_Picture_s;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagBitrateInfo {
    pub iLayer: LAYER_NUM,
    pub iBitrate: ::libc::c_int,
}
impl ::std::default::Default for Struct_TagBitrateInfo {
    fn default() -> Struct_TagBitrateInfo { unsafe { ::std::mem::zeroed() } }
}
pub type SBitrateInfo = Struct_TagBitrateInfo;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagDumpLayer {
    pub iLayer: ::libc::c_int,
    pub pFileName: *mut ::libc::c_char,
}
impl ::std::default::Default for Struct_TagDumpLayer {
    fn default() -> Struct_TagDumpLayer { unsafe { ::std::mem::zeroed() } }
}
pub type SDumpLayer = Struct_TagDumpLayer;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagProfileInfo {
    pub iLayer: ::libc::c_int,
    pub uiProfileIdc: EProfileIdc,
}
impl ::std::default::Default for Struct_TagProfileInfo {
    fn default() -> Struct_TagProfileInfo { unsafe { ::std::mem::zeroed() } }
}
pub type SProfileInfo = Struct_TagProfileInfo;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagLevelInfo {
    pub iLayer: ::libc::c_int,
    pub uiLevelIdc: ELevelIdc,
}
impl ::std::default::Default for Struct_TagLevelInfo {
    fn default() -> Struct_TagLevelInfo { unsafe { ::std::mem::zeroed() } }
}
pub type SLevelInfo = Struct_TagLevelInfo;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagDeliveryStatus {
    pub bDeliveryFlag: _bool,
    pub iDropFrameType: ::libc::c_int,
    pub iDropFrameSize: ::libc::c_int,
}
impl ::std::default::Default for Struct_TagDeliveryStatus {
    fn default() -> Struct_TagDeliveryStatus {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SDeliveryStatus = Struct_TagDeliveryStatus;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagDecoderCapability {
    pub iProfileIdc: ::libc::c_int,
    pub iProfileIop: ::libc::c_int,
    pub iLevelIdc: ::libc::c_int,
    pub iMaxMbps: ::libc::c_int,
    pub iMaxFs: ::libc::c_int,
    pub iMaxCpb: ::libc::c_int,
    pub iMaxDpb: ::libc::c_int,
    pub iMaxBr: ::libc::c_int,
    pub bRedPicCap: _bool,
}
impl ::std::default::Default for Struct_TagDecoderCapability {
    fn default() -> Struct_TagDecoderCapability {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SDecoderCapability = Struct_TagDecoderCapability;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagParserBsInfo {
    pub iNalNum: ::libc::c_int,
    pub iNalLenInByte: [::libc::c_int; 128],
    pub pDstBuff: *mut ::libc::c_uchar,
    pub iSpsWidthInPixel: ::libc::c_int,
    pub iSpsHeightInPixel: ::libc::c_int,
    pub uiInBsTimeStamp: ::libc::c_ulonglong,
    pub uiOutBsTimeStamp: ::libc::c_ulonglong,
}
impl ::std::default::Default for Struct_TagParserBsInfo {
    fn default() -> Struct_TagParserBsInfo { unsafe { ::std::mem::zeroed() } }
}
pub type SParserBsInfo = Struct_TagParserBsInfo;
pub type PParserBsInfo = *mut Struct_TagParserBsInfo;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagVideoEncoderStatistics {
    pub uiWidth: ::libc::c_uint,
    pub uiHeight: ::libc::c_uint,
    pub fAverageFrameSpeedInMs: ::libc::c_float,
    pub fAverageFrameRate: ::libc::c_float,
    pub fLatestFrameRate: ::libc::c_float,
    pub uiBitRate: ::libc::c_uint,
    pub uiAverageFrameQP: ::libc::c_uint,
    pub uiInputFrameCount: ::libc::c_uint,
    pub uiSkippedFrameCount: ::libc::c_uint,
    pub uiResolutionChangeTimes: ::libc::c_uint,
    pub uiIDRReqNum: ::libc::c_uint,
    pub uiIDRSentNum: ::libc::c_uint,
    pub uiLTRSentNum: ::libc::c_uint,
    pub iStatisticsTs: ::libc::c_longlong,
}
impl ::std::default::Default for Struct_TagVideoEncoderStatistics {
    fn default() -> Struct_TagVideoEncoderStatistics {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SEncoderStatistics = Struct_TagVideoEncoderStatistics;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_TagVideoDecoderStatistics {
    pub uiWidth: ::libc::c_uint,
    pub uiHeight: ::libc::c_uint,
    pub fAverageFrameSpeedInMs: ::libc::c_float,
    pub fActualAverageFrameSpeedInMs: ::libc::c_float,
    pub uiDecodedFrameCount: ::libc::c_uint,
    pub uiResolutionChangeTimes: ::libc::c_uint,
    pub uiIDRCorrectNum: ::libc::c_uint,
    pub uiAvgEcRatio: ::libc::c_uint,
    pub uiAvgEcPropRatio: ::libc::c_uint,
    pub uiEcIDRNum: ::libc::c_uint,
    pub uiEcFrameNum: ::libc::c_uint,
    pub uiIDRLostNum: ::libc::c_uint,
    pub uiFreezingIDRNum: ::libc::c_uint,
    pub uiFreezingNonIDRNum: ::libc::c_uint,
    pub iAvgLumaQp: ::libc::c_int,
}
impl ::std::default::Default for Struct_TagVideoDecoderStatistics {
    fn default() -> Struct_TagVideoDecoderStatistics {
        unsafe { ::std::mem::zeroed() }
    }
}
pub type SDecoderStatistics = Struct_TagVideoDecoderStatistics;
pub type ISVCEncoderVtbl = Struct_ISVCEncoderVtbl;
pub type ISVCEncoder = *const ISVCEncoderVtbl;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_ISVCEncoderVtbl {
    pub Initialize: ::std::option::Option<extern "C" fn
                                              (arg1: *mut ISVCEncoder,
                                               pParam: *const SEncParamBase)
                                              -> ::libc::c_int>,
    pub InitializeExt: ::std::option::Option<extern "C" fn
                                                 (arg1: *mut ISVCEncoder,
                                                  pParam: *const SEncParamExt)
                                                 -> ::libc::c_int>,
    pub GetDefaultParams: ::std::option::Option<extern "C" fn
                                                    (arg1: *mut ISVCEncoder,
                                                     pParam:
                                                         *mut SEncParamExt)
                                                    -> ::libc::c_int>,
    pub Uninitialize: ::std::option::Option<extern "C" fn
                                                (arg1: *mut ISVCEncoder)
                                                -> ::libc::c_int>,
    pub EncodeFrame: ::std::option::Option<extern "C" fn
                                               (arg1: *mut ISVCEncoder,
                                                kpSrcPic:
                                                    *const SSourcePicture,
                                                pBsInfo: *mut SFrameBSInfo)
                                               -> ::libc::c_int>,
    pub EncodeParameterSets: ::std::option::Option<extern "C" fn
                                                       (arg1:
                                                            *mut ISVCEncoder,
                                                        pBsInfo:
                                                            *mut SFrameBSInfo)
                                                       -> ::libc::c_int>,
    pub ForceIntraFrame: ::std::option::Option<extern "C" fn
                                                   (arg1: *mut ISVCEncoder,
                                                    bIDR: _bool)
                                                   -> ::libc::c_int>,
    pub SetOption: ::std::option::Option<extern "C" fn
                                             (arg1: *mut ISVCEncoder,
                                              eOptionId: ENCODER_OPTION,
                                              pOption: *mut ::libc::c_void)
                                             -> ::libc::c_int>,
    pub GetOption: ::std::option::Option<extern "C" fn
                                             (arg1: *mut ISVCEncoder,
                                              eOptionId: ENCODER_OPTION,
                                              pOption: *mut ::libc::c_void)
                                             -> ::libc::c_int>,
}
impl ::std::default::Default for Struct_ISVCEncoderVtbl {
    fn default() -> Struct_ISVCEncoderVtbl { unsafe { ::std::mem::zeroed() } }
}
pub type ISVCDecoderVtbl = Struct_ISVCDecoderVtbl;
pub type ISVCDecoder = *mut ISVCDecoderVtbl;
#[repr(C)]
#[derive(Copy)]
pub struct Struct_ISVCDecoderVtbl {
    pub Initialize: extern "C" fn(arg1: *mut ISVCDecoder, pParam: *const SDecodingParam)
                                -> ::libc::c_long,
    pub Uninitialize: extern "C" fn(arg1: *mut ISVCDecoder) -> ::libc::c_long,
    pub DecodeFrame: extern "C" fn(arg1: *mut ISVCDecoder,
                                   pSrc: *const ::libc::c_uchar,
                                   iSrcLen: ::libc::c_int,
                                   ppDst: *mut *mut ::libc::c_uchar,
                                   pStride: *mut ::libc::c_int,
                                   iWidth: *mut ::libc::c_int,
                                   iHeight: *mut ::libc::c_int) -> DECODING_STATE,
    pub DecodeFrameNoDelay: extern "C" fn(arg1: *mut ISVCDecoder,
                                                       pSrc:
                                                           *const ::libc::c_uchar,
                                                       iSrcLen: ::libc::c_int,
                                                       ppDst:
                                                           *mut *mut ::libc::c_uchar,
                                                       pDstInfo:
                                                           *mut SBufferInfo)
                                                      -> DECODING_STATE,
    pub DecodeFrame2: extern "C" fn(arg1: *mut ISVCDecoder,
                                                 pSrc: *const ::libc::c_uchar,
                                                 iSrcLen: ::libc::c_int,
                                                 ppDst:
                                                     *mut *mut ::libc::c_uchar,
                                                 pDstInfo: *mut SBufferInfo)
                                                -> DECODING_STATE,
    pub DecodeParser: extern "C" fn(arg1: *mut ISVCDecoder,
                                                 pSrc: *const ::libc::c_uchar,
                                                 iSrcLen: ::libc::c_int,
                                                 pDstInfo: *mut SParserBsInfo)
                                                -> DECODING_STATE,
    pub DecodeFrameEx: extern "C" fn(arg1: *mut ISVCDecoder,
                                                  pSrc:
                                                      *const ::libc::c_uchar,
                                                  iSrcLen: ::libc::c_int,
                                                  pDst: *mut ::libc::c_uchar,
                                                  iDstStride: ::libc::c_int,
                                                  iDstLen: *mut ::libc::c_int,
                                                  iWidth: *mut ::libc::c_int,
                                                  iHeight: *mut ::libc::c_int,
                                                  iColorFormat:
                                                      *mut ::libc::c_int)
                                                 -> DECODING_STATE,
    pub SetOption: extern "C" fn(arg1: *mut ISVCDecoder,
                                              eOptionId: DECODER_OPTION,
                                              pOption: *mut ::libc::c_void)
                                             -> ::libc::c_long,
    pub GetOption: extern "C" fn(arg1: *mut ISVCDecoder,
                                              eOptionId: DECODER_OPTION,
                                              pOption: *mut ::libc::c_void)
                                             -> ::libc::c_long,
}
impl ::std::default::Default for Struct_ISVCDecoderVtbl {
    fn default() -> Struct_ISVCDecoderVtbl { unsafe { ::std::mem::zeroed() } }
}
pub type WelsTraceCallback =
    ::std::option::Option<extern "C" fn
                              (ctx: *mut ::libc::c_void, level: ::libc::c_int,
                               string: *const ::libc::c_char) -> ()>;

#[link(name="openh264")]
extern "C" {
    pub fn WelsCreateSVCEncoder(ppEncoder: *mut *mut ISVCEncoder)
     -> ::libc::c_int;
    pub fn WelsDestroySVCEncoder(pEncoder: *mut ISVCEncoder) -> ();
    pub fn WelsGetDecoderCapability(pDecCapability: *mut SDecoderCapability)
     -> ::libc::c_int;
    pub fn WelsCreateDecoder(ppDecoder: *mut *mut ISVCDecoder)
     -> ::libc::c_long;
    pub fn WelsDestroyDecoder(pDecoder: *mut ISVCDecoder) -> ();
    pub fn WelsGetCodecVersion() -> OpenH264Version;
    pub fn WelsGetCodecVersionEx(pVersion: *mut OpenH264Version) -> ();
}

}
