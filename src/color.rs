pub trait IColor: Clone {
    fn r(&self) -> u8;
    fn g(&self) -> u8;
    fn b(&self) -> u8;
    fn a(&self) -> u8;

    fn from_other<C: IColor>(other: &C) -> Self {
        Self::from_rgba(other.r(), other.g(), other.b(), other.a())
    }

    fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self;
}
