//! Scalability benchmarks for the Million-Scale AI SoA system
//!
//! Run with: cargo bench --bench scalability

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use orbit_royale_server::game::state::{GameState, MatchPhase, Player, GravityWell};
use orbit_royale_server::game::systems::ai_soa::{AiManagerSoA, AiBehavior};
use orbit_royale_server::util::vec2::Vec2;
use uuid::Uuid;

// ============================================================================
// Test Data Generators
// ============================================================================

fn create_test_state_with_bots(bot_count: usize, human_count: usize) -> (GameState, AiManagerSoA) {
    let mut state = GameState::new();
    state.match_state.phase = MatchPhase::Playing;

    // Add gravity wells
    for i in 0..5 {
        let angle = (i as f32) * std::f32::consts::TAU / 5.0;
        let pos = Vec2::new(angle.cos() * 2000.0, angle.sin() * 2000.0);
        let well = GravityWell::new(i, pos, 10000.0, 50.0);
        state.arena.gravity_wells.insert(i, well);
    }

    let mut manager = AiManagerSoA::with_capacity(bot_count);

    // Add bots distributed around the arena
    for i in 0..bot_count {
        let angle = (i as f32) * std::f32::consts::TAU / (bot_count as f32);
        let radius = 500.0 + (i as f32 % 1000.0);
        let pos = Vec2::new(angle.cos() * radius, angle.sin() * radius);

        let bot = Player {
            id: Uuid::new_v4(),
            name: format!("Bot{}", i),
            position: pos,
            velocity: Vec2::new(angle.sin() * 50.0, -angle.cos() * 50.0),
            rotation: 0.0,
            mass: 100.0,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: true,
            color_index: 0,
            respawn_timer: 0.0,
        };
        let bot_id = bot.id;
        state.add_player(bot);
        manager.register_bot(bot_id);
    }

    // Add human players
    for i in 0..human_count {
        let angle = (i as f32) * std::f32::consts::TAU / (human_count.max(1) as f32);
        let pos = Vec2::new(angle.cos() * 300.0, angle.sin() * 300.0);

        let human = Player {
            id: Uuid::new_v4(),
            name: format!("Human{}", i),
            position: pos,
            velocity: Vec2::ZERO,
            rotation: 0.0,
            mass: 150.0,
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: false,
            color_index: i as u8,
            respawn_timer: 0.0,
        };
        state.add_player(human);
    }

    (state, manager)
}

// ============================================================================
// Registration Benchmarks
// ============================================================================

fn bench_registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("registration");

    for count in [100, 1_000, 10_000, 100_000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::new("register_bots", count),
            count,
            |b, &count| {
                b.iter(|| {
                    let mut manager = AiManagerSoA::with_capacity(count);
                    for _ in 0..count {
                        manager.register_bot(black_box(Uuid::new_v4()));
                    }
                    manager
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Update Cycle Benchmarks
// ============================================================================

fn bench_full_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_update");
    group.sample_size(50);

    for &(bot_count, human_count) in &[
        (100, 1),
        (1_000, 1),
        (10_000, 1),
        (10_000, 10),
        (100_000, 1),
        (100_000, 10),
    ] {
        let label = format!("{}bots_{}humans", bot_count, human_count);
        group.throughput(Throughput::Elements(bot_count as u64));

        group.bench_function(BenchmarkId::new("update", &label), |b| {
            let (state, mut manager) = create_test_state_with_bots(bot_count, human_count);
            b.iter(|| {
                manager.update(black_box(&state), black_box(0.033));
            });
        });
    }

    group.finish();
}

// ============================================================================
// Dormancy Benchmarks
// ============================================================================

fn bench_dormancy_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("dormancy");

    for &bot_count in &[1_000, 10_000, 100_000] {
        group.throughput(Throughput::Elements(bot_count as u64));

        group.bench_function(BenchmarkId::new("update_dormancy", bot_count), |b| {
            let (state, mut manager) = create_test_state_with_bots(bot_count, 10);
            b.iter(|| {
                manager.update_dormancy(black_box(&state));
            });
        });
    }

    group.finish();
}

// ============================================================================
// Zone Update Benchmarks
// ============================================================================

fn bench_zone_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("zones");

    for &bot_count in &[1_000, 10_000, 100_000] {
        group.throughput(Throughput::Elements(bot_count as u64));

        group.bench_function(BenchmarkId::new("update_zones", bot_count), |b| {
            let (state, mut manager) = create_test_state_with_bots(bot_count, 10);
            b.iter(|| {
                manager.update_zones(black_box(&state));
            });
        });
    }

    group.finish();
}

// ============================================================================
// Behavior Batch Benchmarks
// ============================================================================

fn bench_behavior_batches(c: &mut Criterion) {
    let mut group = c.benchmark_group("behavior_batches");

    for &bot_count in &[1_000, 10_000, 100_000] {
        group.throughput(Throughput::Elements(bot_count as u64));

        // Benchmark batch rebuild
        group.bench_function(BenchmarkId::new("rebuild_batches", bot_count), |b| {
            let (state, mut manager) = create_test_state_with_bots(bot_count, 10);
            manager.update_dormancy(&state);

            b.iter(|| {
                manager.batches.rebuild(
                    black_box(&manager.behaviors),
                    black_box(&manager.active_mask),
                );
            });
        });
    }

    group.finish();
}

// ============================================================================
// Input Generation Benchmarks
// ============================================================================

fn bench_input_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("input_generation");

    for &bot_count in &[1_000, 10_000, 100_000] {
        group.throughput(Throughput::Elements(bot_count as u64));

        group.bench_function(BenchmarkId::new("get_all_inputs", bot_count), |b| {
            let (_, manager) = create_test_state_with_bots(bot_count, 1);
            let bot_ids: Vec<_> = manager.bot_ids.clone();

            b.iter(|| {
                for (tick, id) in bot_ids.iter().enumerate() {
                    black_box(manager.get_input(*id, tick as u64));
                }
            });
        });
    }

    group.finish();
}

// ============================================================================
// Memory Layout Benchmarks
// ============================================================================

fn bench_memory_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_access");

    for &bot_count in &[10_000, 100_000] {
        // Sequential access (cache-friendly)
        group.bench_function(BenchmarkId::new("sequential_thrust", bot_count), |b| {
            let (_, manager) = create_test_state_with_bots(bot_count, 1);
            let thrust_x = &manager.thrust_x;
            let thrust_y = &manager.thrust_y;

            b.iter(|| {
                let mut sum = 0.0f32;
                for i in 0..thrust_x.len() {
                    sum += thrust_x[i] + thrust_y[i];
                }
                black_box(sum)
            });
        });
    }

    group.finish();
}

// ============================================================================
// Network/Protocol Benchmarks
// ============================================================================

fn bench_snapshot_creation(c: &mut Criterion) {
    use orbit_royale_server::net::protocol::GameSnapshot;

    let mut group = c.benchmark_group("snapshot_creation");

    for &(bot_count, human_count) in &[(30, 1), (100, 10), (500, 50)] {
        let label = format!("{}bots_{}humans", bot_count, human_count);
        group.throughput(Throughput::Elements((bot_count + human_count) as u64));

        group.bench_function(BenchmarkId::new("from_state", &label), |b| {
            let (state, _) = create_test_state_with_bots(bot_count, human_count);
            b.iter(|| {
                black_box(GameSnapshot::from_game_state(black_box(&state)))
            });
        });
    }

    group.finish();
}

fn bench_spatial_grid(c: &mut Criterion) {
    use orbit_royale_server::game::spatial::{SpatialGrid, SpatialEntity, SpatialEntityId, ENTITY_GRID_CELL_SIZE};

    let mut group = c.benchmark_group("spatial_grid");

    for &entity_count in &[100, 1000, 5000] {
        group.throughput(Throughput::Elements(entity_count as u64));

        // Benchmark insertion
        group.bench_function(BenchmarkId::new("insert", entity_count), |b| {
            b.iter(|| {
                let mut grid = SpatialGrid::new(ENTITY_GRID_CELL_SIZE);
                for i in 0..entity_count {
                    let angle = (i as f32) * std::f32::consts::TAU / (entity_count as f32);
                    let radius = 100.0 + (i as f32 % 500.0);
                    let pos = Vec2::new(angle.cos() * radius, angle.sin() * radius);
                    grid.insert(SpatialEntity {
                        id: SpatialEntityId::Projectile(i as u64),
                        position: pos,
                        radius: 5.0,
                    });
                }
                black_box(grid)
            });
        });

        // Benchmark collision queries
        group.bench_function(BenchmarkId::new("collision_pairs", entity_count), |b| {
            let mut grid = SpatialGrid::new(ENTITY_GRID_CELL_SIZE);
            for i in 0..entity_count {
                let angle = (i as f32) * std::f32::consts::TAU / (entity_count as f32);
                let radius = 100.0 + (i as f32 % 500.0);
                let pos = Vec2::new(angle.cos() * radius, angle.sin() * radius);
                grid.insert(SpatialEntity {
                    id: SpatialEntityId::Projectile(i as u64),
                    position: pos,
                    radius: 5.0,
                });
            }

            b.iter(|| {
                black_box(grid.get_all_collision_pairs())
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_registration,
    bench_full_update,
    bench_dormancy_update,
    bench_zone_updates,
    bench_behavior_batches,
    bench_input_generation,
    bench_memory_access,
    bench_snapshot_creation,
    bench_spatial_grid,
);

criterion_main!(benches);
