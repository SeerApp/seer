use core::panic;

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
            pos: self.pos.clone(),
        }
    }

    pub fn peek(&self, n: usize) -> &[u8] {
        let end = self.pos + n;
        if end > self.buf.len() {
            panic!("Taking length exceeding data buffer from Cursor");
        }
        let slice = &self.buf[self.pos..end];
        slice
    }

    pub fn take(&mut self, n: usize) -> &[u8] {
        let end = self.pos + n;
        if end > self.buf.len() {
            panic!("Taking length exceeding data buffer from Cursor");
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        slice
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

    pub fn set_pos_relative(&mut self, offset: i32) {
        self.set_pos_relative_from(offset, self.pos);
    }

    pub fn set_pos_relative_from(&mut self, offset: i32, from: usize) {
        let len = self.buf.len() as i32;
        let new_pos = from as i32 + offset;

        if new_pos < 0 || new_pos > len {
            panic!("Relative offset out of bounds");
        }

        self.pos = new_pos as usize;
    }
}
