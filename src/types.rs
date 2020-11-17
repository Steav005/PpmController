enum ButtonState {
    Up,
    Neutral,
    Down,
}

/// Channel count starts at 1
///
/// Standard AETR mapping
#[repr(packed)]
struct JoystickState {
    /// Aileron, Channel 1
    x: i16,

    /// Elevator, Channel 2
    y: i16,

    /// Throttle, Channel 3
    throttle: i16,

    /// Rudder, Channel 4
    z: i16,

    /// Channel 5
    c5: i16,

    /// Channel 6
    c6: i16,

    /// Slider
    slider: [u16; 14],
}

impl JoystickState {
    // this is actually safe, as long as `JoystickState` is packed. More information:
    // https://stackoverflow.com/questions/28127165/how-to-convert-struct-to-u8
    /// Return a byte slice to this struct
    unsafe fn as_u8_slice(&self) -> &[u8] {
        ::core::slice::from_raw_parts(
            (self as *const Self) as *const u8,
            ::core::mem::size_of::<Self>(),
        )
    }
}
