pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { buf: data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn clone(&self) -> Self {
        Self {
            buf: self.buf,
            pos: self.pos,
        }
    }

    pub fn peek(&self, n: usize) -> Option<&[u8]> {
        let end = self.pos + n;
        if end > self.buf.len() {
            return None;
        }
        Some(&self.buf[self.pos..end])
    }

    pub fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos + n;
        if end > self.buf.len() {
            return None;
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Some(slice)
    }

    /// Like [`Self::take`], but runs `on_none` with the number of bytes remaining when the buffer is too short.
    pub fn take_or_else<'b>(
        &'b mut self,
        n: usize,
        on_none: impl FnOnce(usize),
    ) -> Option<&'b [u8]> {
        let end = self.pos + n;
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
            len + offset // from end
        };

        self.pos = new_pos.clamp(0, len) as usize;
    }

    pub fn set_pos_relative(&mut self, offset: i32) -> bool {
        self.set_pos_relative_from(offset, self.pos)
    }

    /// Returns `false` if the new position would fall outside the buffer.
    pub fn set_pos_relative_from(&mut self, offset: i32, from: usize) -> bool {
        let len = self.buf.len() as i32;
        let new_pos = from as i32 + offset;

        if new_pos < 0 || new_pos > len {
            return false;
        }

        self.pos = new_pos as usize;
        true
    }
}
