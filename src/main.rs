use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct Elf64SectionHeader {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64, // only text is mapped (there is no mapping lmao)
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
}

fn read_u16(a: &[u8], b: u64) -> u16 {
    u16::from_le_bytes(a[b as usize..(b + 2) as usize].try_into().unwrap())
}

fn read_u32(a: &[u8], b: u64) -> u32 {
    u32::from_le_bytes(a[b as usize..(b + 4) as usize].try_into().unwrap())
}

fn verify_header(header: &Elf64Header) {
    assert_eq!(header.e_ident[0], 0x7f);
    assert_eq!(header.e_ident[1], 0x45);
    assert_eq!(header.e_ident[2], 0x4c);
    assert_eq!(header.e_ident[3], 0x46); // check the other bytes i forgot to
    assert_eq!(header.e_type, 2); // executable
    assert_eq!(header.e_machine, 0xf3);
    assert_eq!(header.e_version, 1);
    assert_eq!(header.e_flags, 5); // EF_RISCV_RVC | EF_RISCV_FLOAT_ABI_DOUBLE
    assert_eq!(header.e_ehsize, size_of::<Elf64Header>() as u16);
    assert_eq!(header.e_phentsize, size_of::<Elf64ProgramHeader>() as u16);
    assert_eq!(header.e_shentsize, size_of::<Elf64SectionHeader>() as u16);
}

fn read_from_slice<T: bytemuck::Pod>(slice: &[u8], index: usize) -> T {
    bytemuck::pod_read_unaligned(&slice[index * size_of::<T>()..(index + 1) * size_of::<T>()])
}

fn read_string_table(table: &[u8], index: usize) -> &str {
    let end = table[index..].iter().position(|&x| x == 0).unwrap() + index;
    std::str::from_utf8(&table[index..end]).unwrap()
}

fn sign_extend(a: u32, b: u32) -> i32 {
    ((a << (32 - b)) as i32) >> (32 - b)
}

fn main() -> std::io::Result<()> {
    let mut file = File::open(std::env::args().nth(1).unwrap())?;
    let mut program = Vec::new();
    file.read_to_end(&mut program)?;

    let mut registers = [0i64; 32]; // x11 = a1, x12 = a2;
    let mut pc = 0u64;

    let header = read_from_slice::<Elf64Header>(&program, 0);
    verify_header(&header);
    let mut program_headers = Vec::<Elf64ProgramHeader>::new();
    for i in 0..header.e_phnum {
        program_headers.push(read_from_slice::<Elf64ProgramHeader>(&program[header.e_phoff as usize..], i as usize));
    }

    // shitty code should work for now

    let mut section_headers = HashMap::<&str, Elf64SectionHeader>::new();
    let string_table_section_header = read_from_slice::<Elf64SectionHeader>(&program[header.e_shoff as usize..],  header.e_shstrndx as usize);
    assert_eq!(string_table_section_header.sh_type, 3); // SHT_STRTAB
    let string_table = &program[string_table_section_header.sh_offset as usize..string_table_section_header.sh_offset as usize + string_table_section_header.sh_size as usize];
    for i in 0..header.e_shnum {
        if i == header.e_shstrndx {
            section_headers.insert(&read_string_table(string_table, string_table_section_header.sh_name as usize), string_table_section_header);
            continue;
        }
        let section_header = read_from_slice::<Elf64SectionHeader>(&program[header.e_shoff as usize..], i as usize);
        section_headers.insert(&read_string_table(string_table, section_header.sh_name as usize), section_header);
    }
    pc = section_headers[".text"].sh_offset;
    while pc <  section_headers[".text"].sh_offset + section_headers[".text"].sh_size {
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
