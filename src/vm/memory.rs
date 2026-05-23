// Single-heap linear memory with GC.
//
// ┌──────────────────────────────────────────────────────────────────┐
// │ [header:2][raw bytes...] │ [header:2][managed obj...] │ ...      │
// └──────────────────────────────────────────────────────────────────┘
//
// Every allocation uses the same backing Vec<u8>. Raw allocations
// (type_tag = 0) have no header and are not tracked by GC.
// Managed allocations (type_tag > 0) have a 2-byte header
// [mark:u8][type_tag:u8] before the payload and are tracked for GC.

pub struct Memory {
    data: Vec<u8>,
    bump: usize,
    pub(crate) free_list: Vec<(usize, usize)>,
    pub(crate) gc_allocations: Vec<(usize, usize)>, // (header_addr, total_size) for managed objs
    gc_threshold: usize,
}

const DEFAULT_GC_THRESHOLD: usize = 65536;

impl Memory {
    pub fn new(initial_size: usize) -> Self {
        Memory {
            data: vec![0; initial_size],
            bump: 0,
            free_list: Vec::new(),
            gc_allocations: Vec::new(),
            gc_threshold: DEFAULT_GC_THRESHOLD,
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

    // ── Raw allocation (no GC tracking) ──────────────────

    /// Allocate raw bytes. No header, not tracked by GC.
    /// Used for constant section, manual buffers.
    pub fn alloc(&mut self, size: usize) -> usize {
        if let Some(pos) = self.free_list.iter().position(|&(_, block_size)| block_size >= size) {
            let (addr, block_size) = self.free_list.swap_remove(pos);
            if block_size > size {
                self.free_list.push((addr + size, block_size - size));
            }
            return addr;
        }
        let end = self.bump + size;
        if end > self.data.len() {
            self.grow(end - self.data.len());
        }
        let ptr = self.bump;
        self.bump = end;
        ptr
    }

    // ── Managed allocation (GC-tracked) ──────────────────

    pub fn needs_gc(&self) -> bool {
        self.bump > self.gc_threshold
    }

    /// Allocate a GC-managed object with a type tag.
    /// Returns pointer to the payload (past the 2-byte header).
    /// Triggers collection if bump exceeds threshold.
    pub fn alloc_managed(&mut self, type_tag: u8, size: usize, roots: &[usize]) -> usize {
        if size == 0 {
            return 0;
        }
        if self.needs_gc() && !roots.is_empty() {
            self.collect_gc(roots);
        }
        let total = size + 2;
        // Try free list (freed by previous GC sweep)
        if let Some(pos) = self.free_list.iter().position(|&(_, block_size)| block_size >= total) {
            let (addr, block_size) = self.free_list.swap_remove(pos);
            if block_size > total {
                self.free_list.push((addr + total, block_size - total));
            }
            // SAFETY: addr is within data, allocated by alloc_managed or sweep
            self.data[addr] = 0;       // mark bit
            self.data[addr + 1] = type_tag;
            self.gc_allocations.push((addr, total));
            return addr + 2;            // payload pointer
        }
        // Bump allocate
        let header = self.bump;
        let end = header + total;
        if end > self.data.len() {
            self.grow(end - self.data.len());
        }
        self.data[header] = 0;
        self.data[header + 1] = type_tag;
        self.bump = end;
        self.gc_allocations.push((header, total));
        header + 2
    }

    pub fn mark(&mut self, payload_ptr: usize) {
        if payload_ptr < 2 {
            return;
        }
        let header = payload_ptr - 2;
        if header + 1 >= self.data.len() {
            return;
        }
        if self.data[header] == 1 {
            return; // already marked
        }
        self.data[header] = 1;
    }

    pub fn sweep(&mut self) {
        let mut surviving = Vec::new();
        for &(header, total) in &self.gc_allocations {
            if header + 1 < self.data.len() && self.data[header] == 1 {
                self.data[header] = 0; // clear mark for next cycle
                surviving.push((header, total));
            } else {
                self.free_list.push((header, total));
            }
        }
        self.gc_allocations = surviving;
    }

    pub fn collect_gc(&mut self, roots: &[usize]) {
        for &root in roots {
            self.mark(root);
        }
        self.sweep();
        let next = self.bump + 4096;
        if next > self.gc_threshold {
            self.gc_threshold = next;
        }
    }

    pub fn managed_type_tag(&self, payload_ptr: usize) -> u8 {
        if payload_ptr < 2 || payload_ptr - 1 >= self.data.len() {
            return 0;
        }
        self.data[payload_ptr - 1]
    }

    pub fn managed_is_marked(&self, payload_ptr: usize) -> bool {
        if payload_ptr < 2 || payload_ptr - 2 >= self.data.len() {
            return false;
        }
        self.data[payload_ptr - 2] == 1
    }

    pub fn reset(&mut self) {
        self.bump = 0;
        self.free_list.clear();
        self.gc_allocations.clear();
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
        assert!(mem.read_checked::<i64>(1).is_none());
    }

    #[test]
    fn test_checked_write() {
        let mut mem = Memory::new(8);
        assert!(mem.write_checked::<i64>(0, 42));
        assert!(!mem.write_checked::<i64>(1, 42));
    }

    #[test]
    fn gc_collects_dead_objects() {
        let mut mem = Memory::new(128);
        let obj1 = mem.alloc_managed(1, 16, &[]);
        let obj2 = mem.alloc_managed(2, 32, &[]);
        let obj3 = mem.alloc_managed(3, 64, &[]);

        assert_eq!(mem.managed_type_tag(obj1), 1);
        assert_eq!(mem.gc_allocations.len(), 3);

        // Only obj2 is a root — others get collected
        mem.collect_gc(&[obj2]);

        assert_eq!(mem.gc_allocations.len(), 1);
        assert_eq!(mem.free_list.len(), 2);
    }

    #[test]
    fn gc_free_list_reuse() {
        let mut mem = Memory::new(128);
        let _obj1 = mem.alloc_managed(1, 16, &[]);
        let obj2 = mem.alloc_managed(2, 16, &[]);
        mem.collect_gc(&[obj2]);
        let obj3 = mem.alloc_managed(3, 16, &[]);
        assert_eq!(mem.managed_type_tag(obj3), 3);
    }

    #[test]
    fn raw_and_managed_coexist() {
        let mut mem = Memory::new(256);
        let raw = mem.alloc(8);
        let obj = mem.alloc_managed(1, 16, &[]);

        unsafe { mem.write::<i64>(raw, 42); }
        assert_eq!(unsafe { mem.read::<i64>(raw) }, 42);

        // Raw allocation is NOT tracked
        assert_eq!(mem.gc_allocations.len(), 1);

        // GC doesn't affect raw allocations
        mem.collect_gc(&[obj]);
        assert_eq!(unsafe { mem.read::<i64>(raw) }, 42);
    }
}
