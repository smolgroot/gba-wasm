pub const EWRAM_SIZE: usize = 256 * 1024;
pub const IWRAM_SIZE: usize = 32 * 1024;
pub const VRAM_SIZE: usize = 96 * 1024;
pub const OAM_SIZE: usize = 1 * 1024;
pub const PALETTE_SIZE: usize = 1 * 1024;

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
}

impl MemoryMap {
    pub fn new() -> Self {
        Self {
            ewram: vec![0; EWRAM_SIZE],
            iwram: vec![0; IWRAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
            palette: vec![0; PALETTE_SIZE],
            io: vec![0; 1 * 1024],
        }
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        match addr {
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize - 0x0200_0000) % EWRAM_SIZE],
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize - 0x0300_0000) % IWRAM_SIZE],
            0x0500_0000..=0x05FF_FFFF => self.palette[(addr as usize - 0x0500_0000) % PALETTE_SIZE],
            0x0600_0000..=0x06FF_FFFF => self.vram[vram_index(addr)],
            0x0700_0000..=0x07FF_FFFF => self.oam[(addr as usize - 0x0700_0000) % OAM_SIZE],
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
            _ => {}
        }
    }

    pub fn read_u16(&self, addr: u32) -> u16 {
        let lo = self.read_u8(addr) as u16;
        let hi = self.read_u8(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    pub fn write_u16(&mut self, addr: u32, value: u16) {
        self.write_u8(addr, (value & 0xFF) as u8);
        self.write_u8(addr.wrapping_add(1), (value >> 8) as u8);
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
}
