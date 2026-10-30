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
//! The import edge (a FileMaker decimal, such as a 7-decimal unit price, rounded
//! to cents; BR-IMPORT-01) is deferred with I3 and not built here.

mod cents;
mod decimal;
mod qty;
pub mod round;
pub mod sales;

pub use cents::Cents;
pub use decimal::ParseError;
pub use qty::{Qty, QTY_DECIMALS, QTY_SCALE};
