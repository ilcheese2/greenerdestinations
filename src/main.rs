use object::read::elf::FileHeader;
use object::{elf, LittleEndian};
use std::fs::File;
use std::io::Read;

fn read_u16(a: &[u8], b: u64) -> u16 {
    u16::from_le_bytes(a[b as usize..(b + 2) as usize].try_into().unwrap())
}

fn read_u32(a: &[u8], b: u64) -> u32 {
    u32::from_le_bytes(a[b as usize..(b + 4) as usize].try_into().unwrap())
}

fn read_from_slice<T: bytemuck::Pod>(slice: &[u8], index: usize) -> T {
    bytemuck::pod_read_unaligned(&slice[index * size_of::<T>()..(index + 1) * size_of::<T>()])
}

fn sign_extend(a: u32, b: u32) -> i32 {
    ((a << (32 - b)) as i32) >> (32 - b)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(std::env::args().nth(1).unwrap())?;
    let mut program = Vec::new();
    file.read_to_end(&mut program)?;

    let elf = elf::FileHeader64::<LittleEndian>::parse(&*program)?;
    let sections = elf.sections(LittleEndian, &*program)?;
    let text = sections.section_by_name(LittleEndian, ".text".as_ref()).unwrap().1;

    let mut registers = [0i64; 32]; // x11 = a1, x12 = a2;
    let mut pc = 0u64;

    // shitty code should work for now
    pc = u64::from(text.sh_offset);
    while pc < u64::from(text.sh_offset) + u64::from(text.sh_size) {
        let instr = read_u16(&program, pc);
        if (instr & 0b11) == 0b11 {
            let instr = read_u32(&program, pc);
            if (instr & 0b1111111) == 0b0111011 { // register register
                let funct7 = instr >> 25;
                let funct3 = (instr >> 12) & 0b111;
                let rd = (instr >> 7) & 0b11111;
                let rs1 = (instr >> 15) & 0b11111;
                let rs2 = (instr >> 20) & 0b11111;
                if funct7 == 0 {
                    if funct3 == 0b000 { // addw
                        // ok to explain casts here usize is bc rust is a pos, i32 is to discard high bits and i64 is to sign extend the result
                        registers[rd as usize] = (registers[rs1 as usize] as i32 + registers[rs2 as usize] as i32) as i64;
                    }
                } else if funct7 == 0b0100000 {
                    if funct3 == 0b000 { // subw
                        registers[rd as usize] = (registers[rs1 as usize] as i32 - registers[rs2 as usize] as i32) as i64;
                    }
                }
            }
            pc += 4;
        } else { // compressed instruction
            if (instr & 0b11) == 0b01 { // https://docs.riscv.org/reference/isa/v20260120/unpriv/c-st-ext.html#27-1-8-rvc-instruction-set-listings
                let funct3 = instr >> 13;
                if funct3 == 0b010 { // li
                    let imm = ((instr >> 2) & 0b11111) | (((instr >> 12) & 1) << 5);
                    let rd = (instr >> 7) & 0b11111;
                    let imm = sign_extend(imm as u32, 6) as i64;
                    registers[rd as usize] = imm;
                }
            }
            pc += 2;
        }
    }
    Ok(())
}
