//! Native slot presentation and numbering. Slot I/O lives in `native_directory`.
mod card;
pub(crate) use card::SaveCard;

pub(crate) const SAVE_PAGE_WIDTH: u32 = 10;
pub(crate) const SAVE_PAGE_COUNT: u32 = 10;
pub(crate) const SAVE_MAX_SLOTS: u32 = SAVE_PAGE_WIDTH * SAVE_PAGE_COUNT;
pub(crate) const MANUAL_SAVE_FIRST_SLOT: u32 = 20;

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;
