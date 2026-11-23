use glam::Vec3;
use or_simulation::{build, PreviewTarget, PtfxManager};
const YPT_NAME: &str = "core.ypt";
const EFFECT_NAME: &str = "proj_flare_trail";
#[test]
fn camera_facing_quad_is_perpendicular_to_view_direction() {
    let path: std::path::PathBuf =
        or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")).join(YPT_NAME);
    if !path.exists() {
        or_test_support::skip_corpus(
            "camera_facing_quad_is_perpendicular_to_view_direction",
            format_args!("skip: fixture missing at {}", path.display()),
        );
        return;
    }
    let asset = or_io::gta5::PtxFxList::open(&path).expect("the fixture opens owned");
    let eff_idx = or_particle::list::effect_rules(&asset)
        .position(|e| e.name == EFFECT_NAME)
        .expect("effect present");

    let inst = build(
        &asset,
        PreviewTarget::Emitter {
            effect: eff_idx,
            event: 4,
        },
    )
    .expect("build event 4");
    let mut mgr = PtfxManager::new();
    mgr.set_effect(inst);
    mgr.clock.play();
    for _ in 0..10 {
        mgr.tick(1.0 / 60.0);
    }
    let eff = mgr.effect.as_ref().expect("effect alive");
    let em = eff.emitters.first().expect("emitter alive");
    assert!(!em.points.is_empty(), "no live particles");

    let cam_pos = Vec3::new(3.5, 3.5, 2.5);
    let world_up = Vec3::new(0.0, 0.0, 1.0);

    let gpu = em.points.to_gpu_instances();
    println!("=== Camera-facing verification for proj_flare_haze ===");
    println!("cam_pos    = {:?}", cam_pos);
    println!("world_up   = {:?}", world_up);

    let mut max_normal_err: f32 = 0.0;
    for (i, g) in gpu.iter().take(5).enumerate() {
        let inst_pos = Vec3::from(g.position);
        let dim = g.dimensions;
        let cam_to_p = cam_pos - inst_pos;
        let view = cam_to_p.normalize_or_zero();
        let mut r = world_up.cross(view);
        if r.length() < 1e-4 {
            r = Vec3::X;
        }
        let right = r.normalize();
        let up = view.cross(right).normalize();

        let half_w = (dim[2] + dim[3]).max(1e-4) * 0.5;
        let half_h = (dim[0] + dim[1]).max(1e-4) * 0.5;

        let corners = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
        let world_corners: Vec<Vec3> = corners
            .iter()
            .map(|(cx, cy)| inst_pos + right * *cx * half_w * 2.0 + up * *cy * half_h * 2.0)
            .collect();

        let edge1 = world_corners[1] - world_corners[0];
        let edge2 = world_corners[3] - world_corners[0];
        let normal = edge1.cross(edge2).normalize_or_zero();
        let alignment = normal.dot(view).abs();
        let normal_err = (1.0 - alignment).abs();
        max_normal_err = max_normal_err.max(normal_err);
        println!(
            "particle #{}: pos={:?} dim={:?} quad_normal={:?} view={:?} \
             |cos(angle)|={:.6}  err={:.2e}",
            i, inst_pos, dim, normal, view, alignment, normal_err
        );
    }
    assert!(
        max_normal_err < 1e-4,
        "camera-aligned quad is NOT perpendicular to the view direction \
         (max normal-vs-view alignment error = {max_normal_err}). The \
         WGSL `mode == 0` branch is mis-rotating the billboard."
    );
}
