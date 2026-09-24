use anchor_lang_idl_spec::IdlDiscriminator;

use crate::cursor::Cursor;

impl<'a> Cursor<'a> {
    pub(crate) fn match_discriminator(&mut self, discriminator: &IdlDiscriminator) -> bool {
        let discriminator_len = discriminator.len();
        if self.remaining() < discriminator_len {
            return false;
        }
        let Some(data_discriminator) = self.peek(discriminator_len) else {
            return false;
        };
        let matched = data_discriminator == discriminator.as_slice();

        if matched && self.take(discriminator_len).is_none() {
            return false;
        }

        matched
    }
}
