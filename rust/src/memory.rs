pub const EWRAM_SIZE: usize = 256 * 1024;
pub const IWRAM_SIZE: usize = 32 * 1024;
pub const VRAM_SIZE: usize = 96 * 1024;
pub const OAM_SIZE: usize = 1 * 1024;
pub const PALETTE_SIZE: usize = 1 * 1024;
pub const IO_BASE: u32 = 0x0400_0000;
pub const IO_SIZE: usize = 1 * 1024;
pub const DISPCNT_ADDR: u32 = IO_BASE;
pub const DISPSTAT_ADDR: u32 = IO_BASE + 0x004;
pub const VCOUNT_ADDR: u32 = IO_BASE + 0x006;
pub const CYCLES_PER_SCANLINE: u32 = 1232;
pub const SCANLINES_PER_FRAME: u32 = 228;
pub const VISIBLE_SCANLINES: u32 = 160;
pub const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME;

const DISPSTAT_IRQ_ENABLE_MASK: u8 = 0b0011_1000;
const DISPSTAT_VBLANK: u8 = 1 << 0;
const DISPSTAT_HBLANK: u8 = 1 << 1;
const DISPSTAT_VCOUNT_MATCH: u8 = 1 << 2;
const HBLANK_START_CYCLE: u32 = 1006;

fn vram_index(addr: u32) -> usize {
    let offset = ((addr - 0x0600_0000) as usize) & 0x1_FFFF;
    if offset >= 0x1_8000 {
        offset - 0x8000
    } else {
        offset
    }
}

#[derive(Debug, Clone)]
pub struct MemoryMap {
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub palette: Vec<u8>,
    pub io: Vec<u8>,
    pub cartridge: Vec<u8>,
    video_cycles: u32,
}

impl MemoryMap {
    pub fn new() -> Self {
        Self {
            ewram: vec![0; EWRAM_SIZE],
            iwram: vec![0; IWRAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
            palette: vec![0; PALETTE_SIZE],
            io: vec![0; IO_SIZE],
            cartridge: Vec::new(),
            video_cycles: 0,
        }
    }

    pub fn load_cartridge(&mut self, data: &[u8]) {
        self.cartridge.clear();
        self.cartridge.extend_from_slice(data);
    }

    pub fn advance_video_cycles(&mut self, cycles: u32) {
        self.video_cycles = (self.video_cycles + cycles % CYCLES_PER_FRAME) % CYCLES_PER_FRAME;
    }

    fn read_io(&self, offset: usize) -> u8 {
        let scanline = self.video_cycles / CYCLES_PER_SCANLINE;
        let scanline_cycle = self.video_cycles % CYCLES_PER_SCANLINE;
        let compare = self.io[5];
        let mut dispstat = self.io[4] & DISPSTAT_IRQ_ENABLE_MASK;

        if scanline >= VISIBLE_SCANLINES && scanline < SCANLINES_PER_FRAME - 1 {
            dispstat |= DISPSTAT_VBLANK;
        }
        if scanline_cycle >= HBLANK_START_CYCLE {
            dispstat |= DISPSTAT_HBLANK;
        }
        if scanline as u8 == compare {
            dispstat |= DISPSTAT_VCOUNT_MATCH;
        }

        match offset {
            0x004 => dispstat,
            0x005 => compare,
            0x006 => scanline as u8,
            0x007 => 0,
            _ => self.io[offset],
        }
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        match addr {
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize - 0x0200_0000) % EWRAM_SIZE],
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize - 0x0300_0000) % IWRAM_SIZE],
            IO_BASE..=0x0400_03FF => self.read_io(addr as usize - IO_BASE as usize),
            0x0500_0000..=0x05FF_FFFF => self.palette[(addr as usize - 0x0500_0000) % PALETTE_SIZE],
            0x0600_0000..=0x06FF_FFFF => self.vram[vram_index(addr)],
            0x0700_0000..=0x07FF_FFFF => self.oam[(addr as usize - 0x0700_0000) % OAM_SIZE],
            0x0800_0000..=0x09FF_FFFF | 0x0A00_0000..=0x0BFF_FFFF | 0x0C00_0000..=0x0DFF_FFFF => {
                let offset = (addr as usize) & 0x01FF_FFFF;
                self.cartridge.get(offset).copied().unwrap_or(0xFF)
            }
            _ => 0,
        }
    }

    pub fn write_u8(&mut self, addr: u32, value: u8) {
        match addr {
            0x0200_0000..=0x02FF_FFFF => {
                let index = (addr as usize - 0x0200_0000) % EWRAM_SIZE;
                self.ewram[index] = value;
            }
            0x0300_0000..=0x03FF_FFFF => {
                let index = (addr as usize - 0x0300_0000) % IWRAM_SIZE;
                self.iwram[index] = value;
            }
            IO_BASE..=0x0400_03FF => {
                let offset = addr as usize - IO_BASE as usize;
                match offset {
                    0x004 => {
                        self.io[offset] = (self.io[offset] & !DISPSTAT_IRQ_ENABLE_MASK)
                            | (value & DISPSTAT_IRQ_ENABLE_MASK);
                    }
                    0x005 => self.io[offset] = value,
                    0x006..=0x007 => {}
                    _ => self.io[offset] = value,
                }
            }
            0x0500_0000..=0x05FF_FFFF => {
                let index = (addr as usize - 0x0500_0000) % PALETTE_SIZE;
                self.palette[index] = value;
            }
            0x0600_0000..=0x06FF_FFFF => {
                self.vram[vram_index(addr)] = value;
            }
            0x0700_0000..=0x07FF_FFFF => {
                let index = (addr as usize - 0x0700_0000) % OAM_SIZE;
                self.oam[index] = value;
            }
            0x0800_0000..=0x0DFF_FFFF => {}
            _ => {}
        }
    }

    pub fn read_u16(&self, addr: u32) -> u16 {
        let lo = self.read_u8(addr) as u16;
        let hi = self.read_u8(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    pub fn read_u32(&self, addr: u32) -> u32 {
        u32::from(self.read_u16(addr)) | (u32::from(self.read_u16(addr.wrapping_add(2))) << 16)
    }

    pub fn write_u16(&mut self, addr: u32, value: u16) {
        self.write_u8(addr, (value & 0xFF) as u8);
        self.write_u8(addr.wrapping_add(1), (value >> 8) as u8);
    }

    pub fn write_u32(&mut self, addr: u32, value: u32) {
        self.write_u16(addr, value as u16);
        self.write_u16(addr.wrapping_add(2), (value >> 16) as u16);
    }
}

impl Default for MemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vram_reads_and_writes_bytes_at_their_offsets() {
        let mut memory = MemoryMap::new();
        memory.write_u8(0x0600_1234, 0xAB);

        assert_eq!(memory.vram[0x1234], 0xAB);
        assert_eq!(memory.read_u8(0x0600_1234), 0xAB);
    }

    #[test]
    fn upper_vram_aperture_mirrors_the_last_32_kib() {
        let mut memory = MemoryMap::new();
        memory.write_u8(0x0601_8000, 0x5A);

        assert_eq!(memory.read_u8(0x0601_0000), 0x5A);
        assert_eq!(memory.read_u8(0x0601_8000), 0x5A);
    }

    #[test]
    fn cartridge_rom_is_mirrored_per_window_and_unloaded_addresses_read_ff() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x12, 0x34, 0x56, 0x78]);
        memory.write_u8(0x0800_0000, 0xFF);

        assert_eq!(memory.read_u8(0x0800_0000), 0x12);
        assert_eq!(memory.read_u8(0x0A00_0000), 0x12);
        assert_eq!(memory.read_u8(0x0C00_0003), 0x78);
        assert_eq!(memory.read_u8(0x0800_0004), 0xFF);
        assert_eq!(memory.read_u8(0x09FF_FFFF), 0xFF);
    }

    #[test]
    fn io_registers_are_visible_only_in_the_documented_io_window() {
        let mut memory = MemoryMap::new();
        memory.write_u16(DISPCNT_ADDR, 0x0403);
        memory.write_u8(0x0400_0400, 0xFF);

        assert_eq!(memory.read_u16(DISPCNT_ADDR), 0x0403);
        assert_eq!(memory.read_u8(0x0400_03FF), 0);
        assert_eq!(memory.read_u8(0x0400_0400), 0);
    }
}
