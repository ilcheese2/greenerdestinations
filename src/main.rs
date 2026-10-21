use object::read::elf::FileHeader;
use object::{elf, LittleEndian};
use std::fs::File;
use std::io::Read;
use object::elf::PT_LOAD;


macro_rules! get_bit {
    ($val:expr, $n:expr) => {
        ((($val) >> ($n)) & 1)
    };
}

macro_rules! get_bits { // https://stackoverflow.com/questions/39321580/fastest-way-to-produce-a-mask-with-n-ones-starting-at-position-i
    ($val:expr, $high:expr, $low:expr) => {
        ((($val) >> ($low)) & ((1 << (($high) - ($low) + 1)) - 1))
    };
}

fn read_u16(a: &[u8], b: u64) -> u16 {
    u16::from_le_bytes(a[b as usize..(b + 2) as usize].try_into().unwrap())
}

fn read_u32(a: &[u8], b: u64) -> u32 {
    u32::from_le_bytes(a[b as usize..(b + 4) as usize].try_into().unwrap())
}

fn read_i32(a: &[u8], b: u64) -> i32 {
    i32::from_le_bytes(a[b as usize..(b + 4) as usize].try_into().unwrap())
}

fn write_i32(a: &mut [u8], b: u64, c: i32) {
    a[b as usize..(b + 4) as usize].copy_from_slice(&c.to_le_bytes());
}

fn read_from_slice<T: bytemuck::Pod>(slice: &[u8], index: usize) -> T {
    bytemuck::pod_read_unaligned(&slice[index * size_of::<T>()..(index + 1) * size_of::<T>()])
}

fn sign_extend(a: u32, b: u32) -> i32 {
    ((a << (32 - b)) as i32) >> (32 - b)
}

struct Cpu {
    memory: Vec<u8>,
    registers: [i64; 32],
    pc: u64,
}

enum Instruction {
    ADD { rd: u8, rs1: u8, rs2: u8 },
    ADDI { rd: u8, rs1: u8, imm: i64 },
    ADDIW { rd: u8, rs1: u8, imm: i32 },
    SUBW { rd: u8, rs1: u8, rs2: u8 },
    SW { rs1: u8, rs2: u8, offset: u16 },
    LW { rs1: u8, rd: u8, offset: u16 },
    Unknown,
}

fn decode_ci(word: u16) -> (u8, u16) {
    (get_bits!(word, 11, 7) as u8, get_bits!(word, 6, 2) | (get_bit!(word, 12) << 5))
}

fn decode_ci_lwsp(word: u16) -> (u8, u16) { // TODO: why does this exist
    (get_bits!(word, 11, 7) as u8, (get_bits!(word, 3, 2) << 6) | (get_bit!(word, 12) << 5) | (get_bits!(word, 6, 4) << 2))
}

fn decode_cl(word: u16) -> (u8, u8, u8) { // TODO: distinguish between RV64C and RV32C
    (8 + get_bits!(word, 9, 7) as u8, (((get_bits!(word, 12, 10) << 1) | get_bit!(word, 6) | (get_bit!(word, 5) << 4)) << 2) as u8, 8 + get_bits!(word, 4, 2) as u8)
}

fn decode_cr(word: u16) -> (u8, u8, u8) {
    (get_bit!(word, 12) as u8, get_bits!(word, 11, 7) as u8, get_bits!(word, 6, 2) as u8)
}

fn decode_css(word: u16) -> (u8, u8) {
    (((get_bits!(word, 12, 9) << 2) | (get_bits!(word, 8, 7) << 6)) as u8, get_bits!(word, 6, 2) as u8)
}

fn decode_r_type(dword: u32) -> (u8, u8, u8, u8, u8) { // fix typing
    (get_bits!(dword, 31, 25) as u8, get_bits!(dword, 24, 20) as u8, get_bits!(dword, 19, 15) as u8, get_bits!(dword, 14, 12) as u8, get_bits!(dword, 11, 7) as u8)
}

fn execute_instruction(cpu: &mut Cpu, instr: Instruction) {
    match instr {
        Instruction::ADD { rd, rs1, rs2 } => {
            cpu.registers[rd as usize] = cpu.registers[rs1 as usize] + cpu.registers[rs2 as usize];
        }
        Instruction::ADDI { rd, rs1, imm } => {
            cpu.registers[rd as usize] = cpu.registers[rs1 as usize] + imm;
        }
        Instruction::ADDIW { rd, rs1, imm } => {
            cpu.registers[rd as usize] = (cpu.registers[rs1 as usize] as i32 + imm)  as i64;
        }
        Instruction::SUBW { rd, rs1, rs2 } => { // ok to explain casts here usize is bc rust is a pos, i32 is to discard high bits and i64 is to sign extend the result
            cpu.registers[rd as usize] = (cpu.registers[rs1 as usize] as i32 - cpu.registers[rs2 as usize] as i32) as i64;
        }
        Instruction::SW { rs1, rs2, offset } => {
            let addr = cpu.registers[rs1 as usize] + offset as i64;
            write_i32(&mut cpu.memory, addr as u64, cpu.registers[rs2 as usize] as i32);
        }
        Instruction::LW { rs1, rd, offset } => {
            let addr = cpu.registers[rs1 as usize] + offset as i64;
            cpu.registers[rd as usize] = read_i32(&cpu.memory, addr as u64) as i64;
        }
        Instruction::Unknown => { },
        _ => {  },
    };
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(std::env::args().nth(1).unwrap())?;
    let mut program = Vec::new();
    file.read_to_end(&mut program)?;

    let elf = elf::FileHeader64::<LittleEndian>::parse(&*program)?;
    let sections = elf.sections(LittleEndian, &*program)?;
    let program_headers = elf.program_headers(LittleEndian, &*program)?;
    let text = sections.section_by_name(LittleEndian, ".text".as_ref()).unwrap().1;

    const STACK_SIZE: usize = 0x10000; // TODO: real mmu
    const HEAP_SIZE: usize = 0x10000;
    let (mut min_vaddr, mut max_vaddr): (usize, usize) = (usize::MAX, 0); // hacky heuristic

    for program_header in program_headers {
        if program_header.p_type.get(LittleEndian) == PT_LOAD {
            let vaddr = program_header.p_vaddr.get(LittleEndian) as usize;
            //min_vaddr = min_vaddr.min(vaddr);
            max_vaddr = max_vaddr.max(vaddr + program_header.p_memsz.get(LittleEndian) as usize);
        }
    }

    let mapped_size = max_vaddr;
    let memory_size = mapped_size + STACK_SIZE + HEAP_SIZE;
    let mut memory: Vec<u8> = vec![0; memory_size];
    for program_header in program_headers {
        if program_header.p_type.get(LittleEndian) == PT_LOAD {
            let vaddr = program_header.p_vaddr.get(LittleEndian) as usize;
            let fsize = program_header.p_filesz.get(LittleEndian) as usize;
            let offset = program_header.p_offset.get(LittleEndian) as usize;
            memory[vaddr..vaddr + fsize].copy_from_slice(&program[offset..offset + fsize]);
            // let msize = program_header.p_memsz.get(LittleEndian) as usize;
            // if msize > fsize {
            //     memory[vaddr + fsize..vaddr + msize].fill(0);
            // }
        }
    }

    let mut registers = [0i64; 32]; // x0 = 0 always, x11 = a1, x12 = a2;
    registers[2] = (max_vaddr + STACK_SIZE) as i64; // x2 = sp

    let mut cpu = Cpu { memory, registers, pc: u64::from(elf.e_entry) };

    // shitty code should work for now
    while cpu.pc < u64::from(elf.e_entry) + u64::from(text.sh_size) {
        let word = read_u16(&cpu.memory, cpu.pc);
        let instr = if (word & 0b11) == 0b11 {
            let dword = read_u32(&cpu.memory, cpu.pc);
            cpu.pc += 4;
            let opcode = get_bits!(dword, 6, 0);
            match opcode {
                0b0111011 => {
                    let (funct7, rs2, rs1, funct3, rd) = decode_r_type(dword);
                    match (funct7, funct3) {
                        (0b0100000, 0b000) => Instruction::SUBW { rd, rs1, rs2 },
                        _ => Instruction::Unknown,
                    }
                }
                _ => Instruction::Unknown,
            }
        } else { // compressed instruction
            cpu.pc += 2;
            let op = get_bits!(word, 1, 0);
            let funct3 = get_bits!(word, 15, 13);
            match op {
                0b01 => match funct3 {
                    0b010 => { // li
                        let (rd, imm) = decode_ci(word);
                        Instruction::ADDI { rd, rs1: 0, imm: sign_extend(imm as u32, 6) as i64 }
                    },
                    0b000 => { // addi
                        let (rd, imm) = decode_ci(word);
                        Instruction::ADDI { rd, rs1: rd, imm: sign_extend(imm as u32, 6) as i64 }
                    }
                    0b001 => { // addiw jal apparently takes the same code point but is 32 only verify that
                        let (rd, imm) = decode_ci(word);
                        Instruction::ADDIW { rd, rs1: rd, imm: sign_extend(imm as u32, 6) }
                    }
                    _ => Instruction::Unknown
                },
                0b00 => match funct3 {
                    0b110 => { // sw
                        let (rs1, offset, rs2) = decode_cl(word);
                        Instruction::SW { rs1, rs2, offset: offset as u16 }
                    }
                    0b010 => { // lw
                        let (rs1, offset, rd) = decode_cl(word);
                        Instruction::LW { rs1, rd, offset: offset as u16 }
                    }
                    _ => Instruction::Unknown,
                }
                0b10 => match funct3 {
                    0b100 => { // j[al]r/mv/add
                        let (bit, rs1, rs2) = decode_cr(word);
                        if bit == 0 {
                            if rs1 != 0 && rs2 != 0 {
                                Instruction::ADD { rd: rs1, rs1: 0, rs2 }
                            } else {
                                Instruction::Unknown
                            }
                        } else {
                            Instruction::Unknown
                        }
                    }
                    0b110 => { // swsp
                        let (offset, rs2) = decode_css(word);
                        Instruction::SW { rs1: 2, rs2, offset: offset as u16 }
                    },
                    0b010 => { // lwsp
                        let (rd, offset) = decode_ci_lwsp(word);
                        Instruction::LW { rs1: 2, rd, offset }
                    }
                    _ => Instruction::Unknown,
                }
                _ => Instruction::Unknown
            }
        };
        execute_instruction(&mut cpu, instr);
    }
    Ok(())
}
