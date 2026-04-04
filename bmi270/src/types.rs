use crate::registers::InternalStatusMessage;

#[derive(Debug)]
pub enum Error<E> {
    Bus(E),
    Asic(InternalStatusMessage),
    Reserved,
}
