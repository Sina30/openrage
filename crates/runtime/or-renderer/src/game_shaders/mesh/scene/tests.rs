use super::super::super::test_pack::gpu;
use super::*;

fn image(rgba: Vec<u8>) -> Image {
    Image {
        width: 4,
        height: 8,
        rgba,
        srgb: false,
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_texture_two_surfaces_name_is_uploaded_once_and_one_that_differs_only_where_no_sample_looks_is_not_taken_for_it(
) {
    let Some((device, queue)) = gpu("a_texture_two_surfaces_name_is_uploaded_once") else {
        return;
    };
    let plain = image(vec![0; 128]);
    let alike = image(vec![0; 128]);
    // A byte no sample of the texels reads
    let mut bytes = vec![0; 128];
    bytes[1] = 1;
    let differs = image(bytes);
    let mut cache = TextureCache::default();
    let first = cache.view(&device, &queue, &plain);
    assert!(cache.view(&device, &queue, &alike) == first);
    assert!(cache.view(&device, &queue, &differs) != first);
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_scene_made_again_takes_the_textures_it_names_again_and_lets_go_of_the_rest() {
    let Some((device, queue)) = gpu("a_scene_made_again_takes_the_textures") else {
        return;
    };
    let (kept, dropped) = (image(vec![1; 128]), image(vec![2; 128]));
    let mut cache = TextureCache::default();
    cache.begin();
    let first = cache.view(&device, &queue, &kept);
    cache.view(&device, &queue, &dropped);
    cache.end();
    assert_eq!(cache.made.values().flatten().count(), 2);
    // The next scene names one of them, and it is the same view
    cache.begin();
    assert!(cache.view(&device, &queue, &kept) == first);
    cache.end();
    assert_eq!(cache.made.values().flatten().count(), 1);
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_cache_of_a_scene_keeps_the_textures_of_the_last_scene_made_and_no_others() {
    use super::super::super::synthetic::{material, pack_on_disk, quad};
    let Some((device, queue)) = gpu("the_cache_of_a_scene_keeps_the_textures") else {
        return;
    };
    let dir = pack_on_disk();
    let pack = GamePack::open(dir.path()).unwrap();
    let target = Target::default();
    let (red, green) = (
        (quad(0.0), material([1.0; 4], Some([255, 0, 0, 255]))),
        (quad(-1.0), material([1.0; 4], Some([0, 255, 0, 255]))),
    );
    let mut cache = PipelineCache::new();
    let mut made = |surfaces: &[(Surface, Material)]| {
        let scene = MeshScene::prepare_with(&mut cache, &device, &queue, &pack, &target, surfaces);
        assert_eq!(scene.drawn(), surfaces.len());
        cache.textures.made.values().flatten().count()
    };
    assert_eq!(made(&[red.clone(), green]), 2);
    assert_eq!(made(std::slice::from_ref(&red)), 1);
    assert_eq!(made(&[red]), 1);
}
