//! Scalability benchmarks for Orbit Royale server
//!
//! Tests performance at various player counts to verify 500-1000+ player target.
//!
//! Run with: cargo bench --bench scalability

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use orbit_royale_server::game::constants::physics::DT;
use orbit_royale_server::game::state::{GameState, MatchPhase, Player};
use orbit_royale_server::game::systems::{collision, gravity, physics};
use orbit_royale_server::util::vec2::Vec2;
use rand::Rng;
use uuid::Uuid;

/// Create a game state with the specified number of randomly distributed players
fn create_state_with_players(count: usize) -> GameState {
    let mut state = GameState::new();
    state.match_state.phase = MatchPhase::Playing;
    let mut rng = rand::thread_rng();

    for i in 0..count {
        // Distribute players randomly across the arena
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let radius = rng.gen_range(100.0..600.0);
        let position = Vec2::new(angle.cos() * radius, angle.sin() * radius);

        // Random velocity
        let velocity = Vec2::new(
            rng.gen_range(-50.0..50.0),
            rng.gen_range(-50.0..50.0),
        );

        let player = Player {
            id: Uuid::new_v4(),
            name: format!("Player{}", i),
            position,
            velocity,
            rotation: rng.gen_range(0.0..std::f32::consts::TAU),
            mass: rng.gen_range(50.0..200.0),
            alive: true,
            kills: 0,
            deaths: 0,
            spawn_protection: 0.0,
            is_bot: true,
            color_index: (i % 8) as u8,
            respawn_timer: 0.0,
        };
        state.add_player(player);
    }

    // Add some projectiles (10% of player count)
    for _ in 0..count / 10 {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let radius = rng.gen_range(100.0..500.0);
        let position = Vec2::new(angle.cos() * radius, angle.sin() * radius);
        let velocity = Vec2::new(
            rng.gen_range(-100.0..100.0),
            rng.gen_range(-100.0..100.0),
        );
        state.add_projectile(Uuid::new_v4(), position, velocity, 10.0);
    }

    state
}

/// Benchmark collision detection at various player counts
fn bench_collision(c: &mut Criterion) {
    let mut group = c.benchmark_group("collision");
    group.sample_size(50);

    for count in [100, 250, 500, 750, 1000] {
        let mut state = create_state_with_players(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("spatial_grid", count),
            &count,
            |b, _| {
                b.iter(|| {
                    black_box(collision::update(&mut state));
                })
            },
        );
    }
    group.finish();
}

/// Benchmark physics updates at various player counts
fn bench_physics(c: &mut Criterion) {
    let mut group = c.benchmark_group("physics");
    group.sample_size(50);

    for count in [100, 250, 500, 750, 1000] {
        let mut state = create_state_with_players(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("parallel", count),
            &count,
            |b, _| {
                b.iter(|| {
                    physics::update(&mut state, black_box(DT));
                })
            },
        );
    }
    group.finish();
}

/// Benchmark gravity calculations at various player counts
fn bench_gravity(c: &mut Criterion) {
    let mut group = c.benchmark_group("gravity");
    group.sample_size(50);

    for count in [100, 250, 500, 750, 1000] {
        let mut state = create_state_with_players(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("multi_well", count),
            &count,
            |b, _| {
                b.iter(|| {
                    gravity::update_central(&mut state, black_box(DT));
                })
            },
        );
    }
    group.finish();
}

/// Benchmark a full game tick (all systems) at various player counts
fn bench_full_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_tick");
    group.sample_size(30);

    for count in [100, 250, 500, 750, 1000] {
        let mut state = create_state_with_players(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("complete", count),
            &count,
            |b, _| {
                b.iter(|| {
                    // Run all systems like the game loop does
                    gravity::update_central(&mut state, DT);
                    physics::update(&mut state, DT);
                    black_box(collision::update(&mut state));
                })
            },
        );
    }
    group.finish();
}

/// Benchmark spatial grid build time
fn bench_spatial_grid(c: &mut Criterion) {
    use orbit_royale_server::game::spatial::{SpatialEntity, SpatialEntityId, SpatialGrid};

    let mut group = c.benchmark_group("spatial_grid");
    group.sample_size(50);

    for count in [100, 500, 1000, 2000] {
        let mut rng = rand::thread_rng();
        let entities: Vec<SpatialEntity> = (0..count)
            .map(|_| {
                let angle = rng.gen_range(0.0..std::f32::consts::TAU);
                let radius = rng.gen_range(100.0..600.0);
                SpatialEntity {
                    id: SpatialEntityId::Player(Uuid::new_v4()),
                    position: Vec2::new(angle.cos() * radius, angle.sin() * radius),
                    radius: rng.gen_range(10.0..30.0),
                }
            })
            .collect();

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("build", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let mut grid = SpatialGrid::new(64.0);
                    for entity in &entities {
                        grid.insert(*entity);
                    }
                    black_box(grid.get_potential_collisions())
                })
            },
        );
    }
    group.finish();
}

/// Performance validation test - ensures tick time stays under budget
fn bench_tick_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("tick_budget");
    group.sample_size(100);
    group.measurement_time(std::time::Duration::from_secs(10));

    // Target: 33.3ms budget at 30 Hz
    // We want to be well under this for headroom

    for count in [500, 750, 1000] {
        let mut state = create_state_with_players(count);

        group.bench_with_input(
            BenchmarkId::new("vs_budget", count),
            &count,
            |b, _| {
                b.iter(|| {
                    gravity::update_central(&mut state, DT);
                    physics::update(&mut state, DT);
                    collision::update(&mut state);
                })
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_collision,
    bench_physics,
    bench_gravity,
    bench_full_tick,
    bench_spatial_grid,
    bench_tick_budget,
);

criterion_main!(benches);
