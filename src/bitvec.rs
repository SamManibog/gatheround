#[derive(Clone, Default)]
pub struct BitVec {
    bytes: Vec<u8>,
    bit_count: usize,
}

impl BitVec {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit_count: 0,
        }
    }

    /// creates a new bitvec with the given number of bits
    pub fn with_size(bit_count: usize) -> Self {
        Self {
            bytes: vec![0; (bit_count + 7) / 8],
            bit_count,
        }
    }

    pub fn len(&self) -> usize {
        self.bit_count
    }

    /// expands the bitvec with more zeros to the specified bit count
    pub fn expand(&mut self, count: usize) {
        if count <= self.bit_count {
            return;
        }

        // fill full byte first
        let unfinished_bits = 8 - (self.bit_count % 8);
        if unfinished_bits != 8 {
            let additional = unfinished_bits.min(count.saturating_sub(self.bit_count));
            self.bit_count += additional;
        }

        if count <= self.bit_count {
            return;
        }

        // add new bytes
        let expansion_bytes = ((count + 7) / 8) as usize;
        self.bytes.resize(expansion_bytes, 0);
        self.bit_count = count;
    }

    /// sets the given bit
    pub fn set(&mut self, index: usize) {
        if index >= self.bit_count {
            return;
        }

        if let Some(byte) = self.bytes.get_mut((index / 8) as usize) {
            let bit_idx = index % 8;

            let bit_mask: u8 = 1 << bit_idx;
            *byte |= bit_mask;
        }
    }

    /// clears the given bit
    pub fn clear(&mut self, index: usize) {
        if index >= self.bit_count {
            return;
        }

        if let Some(byte) = self.bytes.get_mut((index / 8) as usize) {
            let bit_idx = index % 8;

            let bit_mask: u8 = !(1 << bit_idx);
            *byte &= bit_mask;
        }
    }

    /// sets the given bit, returning the original value, if stored
    pub fn set_by_bool(&mut self, index: usize, on: bool) -> Option<bool> {
        if index >= self.bit_count {
            return None;
        }

        let byte = self.bytes.get_mut((index / 8) as usize)?;
        let bit_idx = index % 8;

        // determine original value
        let bit_mask: u8 = 1 << bit_idx;
        let output = *byte & bit_mask != 0;

        // modify the value
        if on {
            *byte |= bit_mask;
        } else {
            *byte &= !bit_mask;
        }

        Some(output)
    }

    /// gets the given bit, if stored
    pub fn test(&self, index: usize) -> Option<bool> {
        if index >= self.bit_count {
            return None;
        }

        let byte = self.bytes.get((index / 8) as usize)?;
        let bit_idx = index % 8;

        let bit_mask: u8 = 1 << bit_idx;
        Some(byte & bit_mask != 0)
    }
}
