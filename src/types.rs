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
#[derive(Copy, Clone, Debug)]
pub struct JoystickState {
    /// Left Stick (Axis 0 and 1)
    pub left_x: i16,
    pub left_y: i16,

    /// Right Stick (Axis 2 and 3)
    pub right_x: i16,
    pub right_y: i16,

    /// Dials
    pub dial_1: i16,
    pub dial_2: i16,

    /// Buttons
    pub buttons: u8,
}

impl JoystickState {
    pub fn from_ppm_time(axes: [PpmTime; 9]) -> Self{
        const PPM_TIME_OFFSET: i16 = -1500;

        let mut buttons: u8 = 0;
        for (i, button_signal) in axes[6..].iter().enumerate().take(3){
            let button_signal = (*button_signal as i16) + PPM_TIME_OFFSET;
            if button_signal > 250{
                buttons |= 1 << (2 * i)
            } else if button_signal < -250{
                buttons |= 1 << ((2 * i) + 1)
            }
        }

        JoystickState{
            left_x: (axes[0] as i16) + PPM_TIME_OFFSET,
            left_y: (axes[1] as i16) + PPM_TIME_OFFSET,
            right_x: (axes[2] as i16) + PPM_TIME_OFFSET,
            right_y: (axes[3] as i16) + PPM_TIME_OFFSET,
            dial_1: (axes[4] as i16) + PPM_TIME_OFFSET,
            dial_2: (axes[5] as i16) + PPM_TIME_OFFSET,
            buttons,
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
