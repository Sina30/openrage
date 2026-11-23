use or_io::container::paging::{DatResource, FIXED_VIRTUAL_BASE};
use or_io::desc::{Image, PagingModel};

const OTHER: PagingModel = PagingModel {
    virtual_base: 0x0010_0000,
    physical_base: 0x0020_0000,
    offset_mask: 0x000F_FFFF,
};

const VIRTUAL_SIZE: usize = 0x40;
const TOTAL_SIZE: usize = 0x60;

fn resource() -> Vec<u8> {
    let mut data = vec![0u8; TOTAL_SIZE];

    data[0x00..0x08].copy_from_slice(&(OTHER.virtual_base + 0x20).to_le_bytes());
    data[0x08..0x10].copy_from_slice(&(OTHER.physical_base + 0x04).to_le_bytes());

    data[0x20..0x24].copy_from_slice(&0xABCD_1234u32.to_le_bytes());
    data[VIRTUAL_SIZE + 4..VIRTUAL_SIZE + 8].copy_from_slice(&0x5566_7788u32.to_le_bytes());

    data
}

#[test]
fn a_resource_with_other_bases_reads_through_both_readers() {
    let data = resource();

    let virtual_ptr = u64::from_le_bytes(data[0x00..0x08].try_into().unwrap());
    let physical_ptr = u64::from_le_bytes(data[0x08..0x10].try_into().unwrap());

    let rsc = DatResource::new(&data, VIRTUAL_SIZE, TOTAL_SIZE - VIRTUAL_SIZE).with_paging(OTHER);
    assert_eq!(rsc.resolve(virtual_ptr).expect("resolves"), Some(0x20));
    assert_eq!(
        rsc.resolve(physical_ptr).expect("resolves"),
        Some(VIRTUAL_SIZE + 4),
        "a physical address still lands past the system half"
    );

    let image = Image::new(&data, VIRTUAL_SIZE).with_paging(OTHER);
    assert_eq!(image.resolve(virtual_ptr).expect("resolves"), 0x20);
    assert_eq!(
        image.resolve(physical_ptr).expect("resolves"),
        VIRTUAL_SIZE + 4
    );

    assert_eq!(
        rsc.resolve(virtual_ptr).unwrap().unwrap(),
        image.resolve(virtual_ptr).unwrap()
    );
    assert_eq!(
        rsc.resolve(physical_ptr).unwrap().unwrap(),
        image.resolve(physical_ptr).unwrap()
    );

    assert_eq!(
        rsc.read_u32(rsc.resolve_required(virtual_ptr).unwrap())
            .unwrap(),
        0xABCD_1234
    );
    let at = image.resolve(physical_ptr).unwrap();
    assert_eq!(
        u32::from_le_bytes(data[at..at + 4].try_into().unwrap()),
        0x5566_7788
    );
}

#[test]
fn a_rage_address_is_not_an_address_under_the_other_model() {
    let data = resource();

    let rage_pointer = FIXED_VIRTUAL_BASE + 0x20;

    let rsc = DatResource::new(&data, VIRTUAL_SIZE, TOTAL_SIZE - VIRTUAL_SIZE).with_paging(OTHER);
    assert!(
        rsc.resolve(rage_pointer).is_err(),
        "0x50000000 belongs to no space in this model and must be refused"
    );

    let image = Image::new(&data, VIRTUAL_SIZE).with_paging(OTHER);
    assert!(image.resolve(rage_pointer).is_err());
}

#[test]
fn the_default_is_still_rage_and_nothing_had_to_ask() {
    let data = vec![0u8; 0x40];
    let rsc = DatResource::new(&data, 0x20, 0x20);

    assert_eq!(rsc.paging(), PagingModel::RAGE);
    assert_eq!(Image::new(&data, 0x20).paging(), PagingModel::RAGE);

    assert_eq!(PagingModel::RAGE.virtual_base, 0x5000_0000);
    assert_eq!(PagingModel::RAGE.physical_base, 0x6000_0000);
    assert_eq!(PagingModel::RAGE.offset_mask, 0x0FFF_FFFF);
}
