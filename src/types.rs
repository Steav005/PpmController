use ppm_decode::PpmTime;

#[allow(dead_code)]
enum ButtonState {
    Up,
    Neutral,
    Down,
}

/// Channel count starts at 1
///
/// Standard AETR mapping
#[repr(packed)]
pub struct JoystickState {
    /// Aileron, Channel 1
    pub x: i16,

    /// Elevator, Channel 2
    pub y: i16,

    /// Throttle, Channel 3
    pub throttle: i16,

    /// Rudder, Channel 4
    pub z: i16,

    /// Channel 5
    pub c5: i16,

    /// Channel 6
    pub c6: i16,

    /// Slider
    pub slider: [i16; 14],
}

impl JoystickState {
    pub fn new(axes: [PpmTime; 20]) -> Self{
        const PPM_TIME_OFFSET: i16 = -1500;

        let mut slider: [i16; 14] = Default::default();
        for (i, s) in axes[6..].iter().enumerate().take(14){
            slider[i] = (*s as i16) - PPM_TIME_OFFSET;
        }

        JoystickState{
            x: (axes[0] as i16) + PPM_TIME_OFFSET,
            y: (axes[1] as i16) + PPM_TIME_OFFSET,
            z: (axes[2] as i16) + PPM_TIME_OFFSET,
            throttle: (axes[3] as i16) + PPM_TIME_OFFSET,
            c5: (axes[4] as i16) + PPM_TIME_OFFSET,
            c6: (axes[5] as i16) + PPM_TIME_OFFSET,
            slider,
        }
    }

    // this is actually safe, as long as `JoystickState` is packed. More information:
    // https://stackoverflow.com/questions/28127165/how-to-convert-struct-to-u8
    /// Return a byte slice to this struct
    pub unsafe fn as_u8_slice(&self) -> &[u8] {
        ::core::slice::from_raw_parts(
            (self as *const Self) as *const u8,
            ::core::mem::size_of::<Self>(),
        )
    }
}
