// growable linear memory
//
// ┌─────────────────────────────────────────────────────────┐
// │ 0x00 │ 0x01 │ ... │ len │ ... │ cap │                   │
// └─────────────────────────────────────────────────────────┘
//   └───────────────────┘     └───────────┘
//         used              reserved (can grow)

pub struct Memory {
    data: Vec<u8>,
    bump: usize,
    free_list: Vec<(usize, usize)>, // (addr, size)
}

impl Memory {
    pub fn new(initial_size: usize) -> Self {
        Memory {
            data: vec![0; initial_size],
            bump: 0,
            free_list: Vec::new(),
        }
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn grow(&mut self, bytes: usize) -> usize {
        let old = self.data.len();
        self.data.resize(old + bytes, 0);
        old
    }

    pub fn alloc(&mut self, size: usize) -> usize {
        // Try to find a free block that fits
        if let Some(pos) = self.free_list.iter().position(|&(_, block_size)| block_size >= size) {
            let (addr, block_size) = self.free_list.swap_remove(pos);
            if block_size > size {
                // Split: put remainder back
                self.free_list.push((addr + size, block_size - size));
            }
            return addr;
        }
        // Fall back to bump allocation
        let end = self.bump + size;
        if end > self.data.len() {
            self.grow(end - self.data.len());
        }
        let ptr = self.bump;
        self.bump = end;
        ptr
    }

    pub fn free(&mut self, ptr: usize, size: usize) {
        self.free_list.push((ptr, size));
    }

    pub fn reset(&mut self) {
        self.bump = 0;
        self.free_list.clear();
    }

    pub fn bump_ptr(&self) -> usize {
        self.bump
    }

    // ─────────────────────────────────────────
    // generic access (unsafe, no bounds check)
    // ─────────────────────────────────────────

    /// Reads a plain-old-data value from a byte address without checking bounds.
    ///
    /// # Safety
    ///
    /// The caller must ensure `addr..addr + size_of::<T>()` is inside the memory
    /// allocation. The returned bytes must be a valid bit pattern for `T`.
    #[inline]
    pub unsafe fn read<T: Copy>(&self, addr: usize) -> T {
        unsafe { (self.data.as_ptr().add(addr) as *const T).read_unaligned() }
    }

    /// Writes a plain-old-data value to a byte address without checking bounds.
    ///
    /// # Safety
    ///
    /// The caller must ensure `addr..addr + size_of::<T>()` is inside the memory
    /// allocation.
    #[inline]
    pub unsafe fn write<T: Copy>(&mut self, addr: usize, val: T) {
        unsafe { (self.data.as_mut_ptr().add(addr) as *mut T).write_unaligned(val) };
    }

    // ─────────────────────────────────────────
    // checked access (safe, with bounds check)
    // ─────────────────────────────────────────

    pub fn read_checked<T: Copy>(&self, addr: usize) -> Option<T> {
        if addr + size_of::<T>() <= self.data.len() {
            // SAFETY: bounds check above guarantees the read is within allocation
            Some(unsafe { self.read(addr) })
        } else {
            None
        }
    }

    pub fn write_checked<T: Copy>(&mut self, addr: usize, val: T) -> bool {
        if addr + size_of::<T>() <= self.data.len() {
            // SAFETY: bounds check above guarantees the write is within allocation
            unsafe { self.write(addr, val) };
            true
        } else {
            false
        }
    }

    // ─────────────────────────────────────────
    // raw access
    // ─────────────────────────────────────────

    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let mem = Memory::new(1024);
        assert_eq!(mem.size(), 1024);
    }

    #[test]
    fn test_grow() {
        let mut mem = Memory::new(1024);
        let old = mem.grow(512);
        assert_eq!(old, 1024);
        assert_eq!(mem.size(), 1536);
    }

    #[test]
    fn test_read_write_i64() {
        let mut mem = Memory::new(64);
        unsafe {
            mem.write::<i64>(0, -42);
            assert_eq!(mem.read::<i64>(0), -42);
        }
    }

    #[test]
    fn test_read_write_f64() {
        let mut mem = Memory::new(64);
        unsafe {
            mem.write::<f64>(0, 1.25);
            assert_eq!(mem.read::<f64>(0), 1.25);
        }
    }

    #[test]
    fn test_multiple_values() {
        let mut mem = Memory::new(64);
        unsafe {
            mem.write::<i64>(0, 111);
            mem.write::<i64>(8, 222);
            mem.write::<i32>(16, 333);
            mem.write::<i16>(20, 444);
            mem.write::<i8>(22, 55);

            assert_eq!(mem.read::<i64>(0), 111);
            assert_eq!(mem.read::<i64>(8), 222);
            assert_eq!(mem.read::<i32>(16), 333);
            assert_eq!(mem.read::<i16>(20), 444);
            assert_eq!(mem.read::<i8>(22), 55);
        }
    }

    #[test]
    fn test_checked_read() {
        let mem = Memory::new(8);
        assert!(mem.read_checked::<i64>(0).is_some());
        assert!(mem.read_checked::<i64>(1).is_none()); // out of bounds
    }

    #[test]
    fn test_checked_write() {
        let mut mem = Memory::new(8);
        assert!(mem.write_checked::<i64>(0, 42));
        assert!(!mem.write_checked::<i64>(1, 42)); // out of bounds
    }
}
