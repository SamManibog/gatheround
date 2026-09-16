#[derive(Debug, Clone, Default)]
pub struct BitSet {
    bytes: Vec<u8>,
    len: usize,
}

impl BitSet {
    pub fn new() -> Self {
        Self::with_capacity(8)
    }

    /// creates a new bitvec with the given number of bits
    pub fn with_capacity(bit_count: usize) -> Self {
        Self {
            bytes: vec![0; bit_count.div_ceil(8)],
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// gets the union of this bit vec and another
    pub fn union(&mut self, other: &Self) {
        self.expand(other.bytes.len());

        for (byte, other_byte) in self.bytes.iter_mut().zip(other.bytes.iter()) {
            let initial_set_count = byte.count_ones() as usize;
            *byte |= other_byte;
            let final_set_count = byte.count_ones() as usize;

            self.len += final_set_count - initial_set_count;
        }
    }

    /// gets the intersection of this bit vec and another
    pub fn intersection(&mut self, other: &Self) {
        for (byte, other_byte) in self.bytes.iter_mut().zip(other.bytes.iter()) {
            let initial_set_count = byte.count_ones() as usize;
            *byte &= other_byte;
            let final_set_count = byte.count_ones() as usize;

            self.len -= initial_set_count + final_set_count;
        }
    }

    /// expands the bitvec to the given number of bytes
    fn expand(&mut self, byte_count: usize) {
        if byte_count <= self.bytes.len() {
            return;
        }
        self.bytes.resize(byte_count, 0);
    }

    /// sets the given bit
    pub fn set(&mut self, index: usize) {
        self.expand(index.div_ceil(8));

        let byte = &mut self.bytes[index / 8];
        let bit_idx = index % 8;

        let bit_mask: u8 = 1 << bit_idx;
        *byte |= bit_mask;
    }

    /// clears the given bit
    pub fn clear(&mut self, index: usize) {
        if index / 8 >= self.bytes.len() {
            return;
        }

        let byte = &mut self.bytes[index / 8];
        let bit_idx = index % 8;

        let bit_mask: u8 = !(1 << bit_idx);
        *byte &= bit_mask;
    }

    /// gets the given bit, if stored
    pub fn test(&self, index: usize) -> bool {
        if index / 8 >= self.bytes.len() {
            return false;
        }

        let byte = self.bytes[index / 8];
        let bit_idx = index % 8;
        let bit_mask: u8 = 1 << bit_idx;
        byte & bit_mask != 0
    }
}
