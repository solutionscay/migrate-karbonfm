//! Money for KarbonFM as integer cents (OD-04-02; spike S-03).
//!
//! Every money column is an INTEGER of cents and all arithmetic is on integers.
//! Text exists only at the edges:
//!
//! * form handlers parse dollar text with [`Cents::parse`] and quantities with
//!   [`Qty::parse`];
//! * templates format with `Display` (`{{ amount }}` gives "$1,234.56") and fill
//!   input fields with [`Cents::to_input_value`];
//! * the sales rules in [`sales`] round half away from zero, as FileMaker Round()
//!   does (BR-SALES-01, BR-SALES-02).
//!
//! * the import edge reads a FileMaker decimal (a 7-decimal unit price, a quantity
//!   with more than 4 places) with [`filemaker::cents`] and [`filemaker::qty`],
//!   rounded half away from zero, and says whether the value changed (BR-IMPORT-01).

mod cents;
mod decimal;
pub mod filemaker;
mod qty;
pub mod round;
pub mod sales;

pub use cents::Cents;
pub use decimal::ParseError;
pub use qty::{Qty, QTY_DECIMALS, QTY_SCALE};
