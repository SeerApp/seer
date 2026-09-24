#[derive(Clone)]
pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
    base_offset: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            buf: data,
            pos: 0,
            base_offset: 0,
        }
    }

    pub fn new_with_base(data: &'a [u8], base_offset: usize) -> Self {
        Self {
            buf: data,
            pos: 0,
            base_offset,
        }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn absolute_pos(&self) -> usize {
        self.base_offset.saturating_add(self.pos)
    }

    pub fn peek(&self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.buf.len() {
            return None;
        }
        Some(&self.buf[self.pos..end])
    }

    pub fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.buf.len() {
            return None;
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Some(slice)
    }

    /// Like [`Self::take`], but runs `on_none` with the number of bytes remaining when the buffer is too short.
    pub fn take_or_else(&mut self, n: usize, on_none: impl FnOnce(usize)) -> Option<&[u8]> {
        let Some(end) = self.pos.checked_add(n) else {
            on_none(self.remaining());
            return None;
        };
        if end > self.buf.len() {
            let rem = self.remaining();
            on_none(rem);
            return None;
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Some(slice)
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn remaining_bytes(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn set_pos_absolute(&mut self, offset: i32) {
        let len = self.buf.len() as i32;

        let new_pos = if offset >= 0 {
            offset
        } else {
            len.saturating_add(offset)
        };

        self.pos = new_pos.clamp(0, len) as usize;
    }

    pub fn set_pos_relative(&mut self, offset: i32) -> bool {
        self.set_pos_relative_from(offset, self.pos)
    }

    /// Returns `false` if the new position would fall outside the buffer.
    pub fn set_pos_relative_from(&mut self, offset: i32, from: usize) -> bool {
        let len = self.buf.len() as i32;
        let new_pos = (from as i32).saturating_add(offset);

        if new_pos < 0 || new_pos > len {
            return false;
        }

        self.pos = new_pos as usize;
        true
    }
}
