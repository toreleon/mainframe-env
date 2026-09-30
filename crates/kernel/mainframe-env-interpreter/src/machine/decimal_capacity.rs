use super::{Decimal, LayoutMetadata};

pub(super) fn decimal_exceeds_picture(layout: &LayoutMetadata, value: Decimal) -> bool {
    layout.digits > 0 && value.coefficient.unsigned_abs().to_string().len() > layout.digits
}
