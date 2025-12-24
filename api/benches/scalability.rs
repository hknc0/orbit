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

/// Benchmark AOI filtering at various player counts
fn bench_aoi_filtering(c: &mut Criterion) {
    use orbit_royale_server::net::aoi::{AOIConfig, AOIManager};
    use orbit_royale_server::game::state::MatchPhase;
    use orbit_royale_server::net::protocol::{GameSnapshot, PlayerSnapshot};

    let mut group = c.benchmark_group("aoi");
    group.sample_size(50);

    for count in [50, 100, 250, 500] {
        let aoi = AOIManager::new(AOIConfig::default());
        let mut rng = rand::thread_rng();

        // Create a test snapshot with many players
        let players: Vec<PlayerSnapshot> = (0..count)
            .map(|i| {
                let angle = rng.gen_range(0.0..std::f32::consts::TAU);
                let radius = rng.gen_range(100.0..800.0);
                PlayerSnapshot {
                    id: Uuid::new_v4(),
                    name: format!("P{}", i),
                    position: Vec2::new(angle.cos() * radius, angle.sin() * radius),
                    velocity: Vec2::new(rng.gen_range(-50.0..50.0), rng.gen_range(-50.0..50.0)),
                    rotation: 0.0,
                    mass: 100.0,
                    alive: true,
                    kills: rng.gen_range(0..10),
                    deaths: 0,
                    spawn_protection: false,
                    is_bot: true,
                    color_index: 0,
                }
            })
            .collect();

        let player_id = players[0].id;
        let player_pos = players[0].position;
        let player_vel = players[0].velocity;

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players,
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: count as u32,
            total_alive: count as u32,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
        };

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("filter", count),
            &count,
            |b, _| {
                b.iter(|| {
                    black_box(aoi.filter_for_player(player_id, player_pos, player_vel, &snapshot))
                })
            },
        );
    }
    group.finish();
}

/// Benchmark protocol encoding performance
fn bench_encoding(c: &mut Criterion) {
    use orbit_royale_server::net::game_session::encode_pooled;
    use orbit_royale_server::net::protocol::{GameSnapshot, PlayerSnapshot, ServerMessage};
    use orbit_royale_server::game::state::MatchPhase;

    let mut group = c.benchmark_group("encoding");
    group.sample_size(100);

    // Create snapshots of various sizes
    for count in [10, 50, 100, 200] {
        let mut rng = rand::thread_rng();

        let players: Vec<PlayerSnapshot> = (0..count)
            .map(|i| {
                PlayerSnapshot {
                    id: Uuid::new_v4(),
                    name: format!("Player{}", i),
                    position: Vec2::new(rng.gen_range(-500.0..500.0), rng.gen_range(-500.0..500.0)),
                    velocity: Vec2::new(rng.gen_range(-50.0..50.0), rng.gen_range(-50.0..50.0)),
                    rotation: rng.gen_range(0.0..6.28),
                    mass: rng.gen_range(50.0..200.0),
                    alive: true,
                    kills: rng.gen_range(0..20),
                    deaths: rng.gen_range(0..5),
                    spawn_protection: false,
                    is_bot: rng.gen_bool(0.5),
                    color_index: rng.gen_range(0..8),
                }
            })
            .collect();

        let snapshot = GameSnapshot {
            tick: 100,
            match_phase: MatchPhase::Playing,
            match_time: 60.0,
            countdown: 0.0,
            players,
            projectiles: vec![],
            debris: vec![],
            arena_collapse_phase: 0,
            arena_safe_radius: 800.0,
            arena_scale: 1.0,
            gravity_wells: vec![],
            total_players: count as u32,
            total_alive: count as u32,
            density_grid: vec![],
            notable_players: vec![],
            echo_client_time: 0,
        };

        let message = ServerMessage::Snapshot(snapshot);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("pooled", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let encoded = encode_pooled(&message).unwrap();
                    black_box(encoded.len())
                })
            },
        );
    }
    group.finish();
}

/// Benchmark input validation (anticheat)
#[cfg(feature = "anticheat")]
fn bench_input_validation(c: &mut Criterion) {
    use orbit_royale_server::anticheat::validator::InputValidator;
    use orbit_royale_server::net::protocol::PlayerInput;

    let mut group = c.benchmark_group("anticheat");
    group.sample_size(1000);

    let validator = InputValidator::default();
    let mut rng = rand::thread_rng();

    // Create a batch of valid inputs
    let inputs: Vec<PlayerInput> = (0..100)
        .map(|i| PlayerInput {
            sequence: i,
            tick: 100 + i,
            client_time: 0,
            thrust: Vec2::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0)).normalize(),
            aim: Vec2::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0)).normalize(),
            boost: rng.gen_bool(0.1),
            fire: rng.gen_bool(0.1),
            fire_released: false,
        })
        .collect();

    group.bench_function("validate_input", |b| {
        let mut idx = 0;
        b.iter(|| {
            let input = &inputs[idx % inputs.len()];
            idx += 1;
            black_box(validator.validate_input(input))
        })
    });

    group.bench_function("validate_sequence", |b| {
        let mut prev_seq = 0u64;
        b.iter(|| {
            prev_seq += 1;
            black_box(validator.validate_sequence(prev_seq - 1, prev_seq))
        })
    });

    group.finish();
}

#[cfg(not(feature = "anticheat"))]
fn bench_input_validation(_c: &mut Criterion) {
    // No-op when anticheat is disabled
}

criterion_group!(
    benches,
    bench_collision,
    bench_physics,
    bench_gravity,
    bench_full_tick,
    bench_spatial_grid,
    bench_tick_budget,
    bench_aoi_filtering,
    bench_encoding,
    bench_input_validation,
);

criterion_main!(benches);
