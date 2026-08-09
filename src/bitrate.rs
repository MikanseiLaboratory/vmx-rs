//! Adaptive bitrate / quality adjustment.

use crate::tables::{
    BITRATE_TABLE, BR_MINQ, BR_PROFILE, BR_RESOLUTION, BR_SHIFT, BR_TARGET, BR_THREADS,
};
use crate::types::{MAX_Q, Profile};

pub fn calculate_bitrate(target_mbps: i32, min: bool) -> i32 {
    let mut t = target_mbps as f32;
    t /= 60.0 * 8.0;
    t *= 1_048_576.0;
    if min {
        t *= 0.95;
    } else {
        t *= 1.05;
    }
    t as i32
}

pub struct BitrateConfig {
    pub min_quality: i32,
    pub dc_shift: i32,
    pub threads: i32,
    pub target_bytes_min: i32,
    pub target_bytes_max: i32,
}

pub fn lookup_bitrate(profile: Profile, height: i32) -> BitrateConfig {
    let profile_i = profile as i32;
    let mut cfg = BitrateConfig {
        min_quality: 80,
        dc_shift: 0,
        threads: 2,
        target_bytes_min: calculate_bitrate(72, true),
        target_bytes_max: calculate_bitrate(72, false),
    };
    for row in BITRATE_TABLE.iter() {
        if row[BR_PROFILE] == profile_i && height >= row[BR_RESOLUTION] {
            cfg.min_quality = row[BR_MINQ];
            cfg.dc_shift = row[BR_SHIFT];
            cfg.threads = row[BR_THREADS];
            cfg.target_bytes_min = calculate_bitrate(row[BR_TARGET], true);
            cfg.target_bytes_max = calculate_bitrate(row[BR_TARGET], false);
            break;
        }
    }
    cfg
}

/// Match `VMX_AdjustBitrate` from libvmx.
pub fn adjust_bitrate(
    quality: &mut i32,
    min_quality: i32,
    frame_len: i32,
    target_min: i32,
    target_max: i32,
) {
    if frame_len == 0 || target_min == 0 || target_max == 0 {
        return;
    }
    let qual = *quality;
    if frame_len < target_min {
        if qual < min_quality {
            *quality = min_quality;
        } else if qual < 76 {
            *quality = (qual + 4).min(MAX_Q);
        } else if qual < 92 {
            *quality = (qual + 2).min(MAX_Q);
        } else if qual < 99 {
            *quality = (qual + 1).min(MAX_Q);
        }
    } else if frame_len > target_max {
        if qual > 92 {
            *quality = qual - 1;
        } else if qual > min_quality {
            *quality = (qual - 2).max(min_quality);
        } else {
            *quality = min_quality;
        }
    }
}
