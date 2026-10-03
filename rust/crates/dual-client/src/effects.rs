use dual_sim::{FrameSnapshot, Side, SimEvent, TacticalEventKind, Vec2};
use macroquad::prelude::{Color, DrawRectangleParams, draw_circle_lines, draw_rectangle_ex, vec2};

const PARTICLE_FRICTION: f32 = 0.98;
const PARTICLE_SQUARE_ROTATION_SPEED: f32 = 1.5 * std::f32::consts::TAU / 60.0;
const MAX_PARTICLES: usize = 1_024;

/// Owns presentation-only particles; simulation events are consumed once by frame number.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct VisualEffects {
    particles: Vec<EffectParticle>,
    last_frame: Option<u64>,
    random_state: u64,
}

impl Default for VisualEffects {
    fn default() -> Self {
        Self {
            particles: Vec::with_capacity(160),
            last_frame: None,
            random_state: 0x6475_616c_2d65_6666,
        }
    }
}

impl VisualEffects {
    /// Converts this completed rules frame's immutable events into short-lived visuals.
    pub(super) fn advance(&mut self, snapshot: &FrameSnapshot) {
        if self.last_frame == Some(snapshot.frame) {
            return;
        }
        self.last_frame = Some(snapshot.frame);

        for event in &snapshot.events {
            match event {
                SimEvent::ShortbowFired { attacker } => {
                    self.spawn_weapon_burst(
                        snapshot.players[attacker.index()].position,
                        *attacker,
                        5,
                        Color::from_rgba(224, 232, 240, 255),
                    );
                }
                SimEvent::LongbowFired { attacker } => {
                    self.spawn_weapon_burst(
                        snapshot.players[attacker.index()].position,
                        *attacker,
                        14,
                        Color::from_rgba(232, 96, 88, 255),
                    );
                }
                SimEvent::LongbowChargeReady { attacker } => {
                    self.spawn_ring(
                        snapshot.players[attacker.index()].position,
                        40.0,
                        4.0,
                        18,
                        Color::from_rgba(232, 96, 88, 255),
                    );
                }
                SimEvent::ShortbowHit { target, .. } => {
                    self.spawn_impact(
                        snapshot.players[target.index()].position,
                        ImpactStyle {
                            ring_size: 36.0,
                            ring_weight: 2.5,
                            ring_lifespan: 10,
                            shard_count: 10,
                            shard_size: 5.0,
                            minimum_speed: 1.5,
                            maximum_speed: 4.5,
                            shard_lifespan: 18,
                            color: Color::from_rgba(232, 192, 96, 255),
                        },
                    );
                }
                SimEvent::LongbowHit { target, .. } => {
                    let position = snapshot.players[target.index()].position;
                    self.spawn_ring(position, 52.0, 4.0, 18, Color::from_rgba(192, 64, 64, 255));
                    self.spawn_shards(
                        position,
                        ShardStyle {
                            count: 50,
                            size: 16.0,
                            minimum_speed: 2.0,
                            maximum_speed: 10.0,
                            lifespan: 120,
                            color: Color::from_rgba(24, 24, 28, 255),
                        },
                    );
                }
                SimEvent::ArrowIntercepted { position } => {
                    self.spawn_impact(
                        *position,
                        ImpactStyle {
                            ring_size: 24.0,
                            ring_weight: 3.0,
                            ring_lifespan: 12,
                            shard_count: 12,
                            shard_size: 5.0,
                            minimum_speed: 2.0,
                            maximum_speed: 6.0,
                            shard_lifespan: 18,
                            color: Color::from_rgba(96, 208, 232, 255),
                        },
                    );
                }
                SimEvent::CoverImpact { position, normal } => {
                    self.spawn_cover_impact(*position, *normal);
                }
                SimEvent::Tactical(event) if event.kind == TacticalEventKind::Disrupt => {
                    if let Some(attacker) = event.attacker {
                        let target = attacker.opponent();
                        self.spawn_impact(
                            snapshot.players[target.index()].position,
                            ImpactStyle {
                                ring_size: 36.0,
                                ring_weight: 3.0,
                                ring_lifespan: 8,
                                shard_count: 8,
                                shard_size: 5.0,
                                minimum_speed: 1.5,
                                maximum_speed: 4.5,
                                shard_lifespan: 18,
                                color: Color::from_rgba(232, 192, 96, 255),
                            },
                        );
                    }
                }
                SimEvent::Tactical(_) => {}
            }
        }

        for particle in &mut self.particles {
            particle.advance();
        }
        self.particles.retain(EffectParticle::is_alive);
    }

    /// Drops particles on a mode or round reset while preserving their local random stream.
    pub(super) fn clear(&mut self) {
        self.particles.clear();
        self.last_frame = None;
    }

    /// Draws particles in the same logical arena transform as players and projectiles.
    pub(super) fn draw(&self, left: f32, top: f32, scale: f32) {
        for particle in &self.particles {
            particle.draw(left, top, scale);
        }
    }

    fn spawn_weapon_burst(&mut self, position: Vec2, attacker: Side, count: usize, color: Color) {
        let player_angle = if attacker == Side::One {
            -std::f32::consts::FRAC_PI_2
        } else {
            std::f32::consts::FRAC_PI_2
        };
        for _ in 0..count {
            let spread = (self.next_unit() - 0.5) * std::f32::consts::FRAC_PI_2;
            let speed = 1.0 + self.next_unit() * 3.0;
            let rotation = self.next_unit() * std::f32::consts::TAU;
            self.push(EffectParticle::square(
                position.plus(Vec2::from_angle(player_angle).scale(18.0)),
                Vec2::from_angle(player_angle + spread).scale(speed),
                3.0,
                12,
                color,
                rotation,
            ));
        }
    }

    fn spawn_impact(&mut self, position: Vec2, style: ImpactStyle) {
        self.spawn_ring(
            position,
            style.ring_size,
            style.ring_weight,
            style.ring_lifespan,
            style.color,
        );
        self.spawn_shards(
            position,
            ShardStyle {
                count: style.shard_count,
                size: style.shard_size,
                minimum_speed: style.minimum_speed,
                maximum_speed: style.maximum_speed,
                lifespan: style.shard_lifespan,
                color: style.color,
            },
        );
    }

    fn spawn_ring(
        &mut self,
        position: Vec2,
        size: f32,
        stroke_weight: f32,
        lifespan: u16,
        color: Color,
    ) {
        self.push(EffectParticle::ring(
            position,
            size,
            stroke_weight,
            lifespan,
            color,
        ));
    }

    fn spawn_shards(&mut self, position: Vec2, style: ShardStyle) {
        for _ in 0..style.count {
            let angle = self.next_unit() * std::f32::consts::TAU;
            let speed = style.minimum_speed
                + self.next_unit() * (style.maximum_speed - style.minimum_speed);
            let rotation = self.next_unit() * std::f32::consts::TAU;
            self.push(EffectParticle::square(
                position,
                Vec2::from_angle(angle).scale(speed),
                style.size,
                style.lifespan,
                style.color,
                rotation,
            ));
        }
    }

    fn spawn_cover_impact(&mut self, position: Vec2, normal: Vec2) {
        self.spawn_ring(position, 9.0, 2.0, 11, Color::from_rgba(224, 232, 240, 255));
        let normal_angle = normal.y.atan2(normal.x);
        for index in 0..6 {
            let spread = index as f32 / 5.0;
            let angle =
                normal_angle - std::f32::consts::FRAC_PI_4 + spread * std::f32::consts::FRAC_PI_2;
            let speed = 2.0 + 0.4 * (index % 3) as f32;
            let rotation = self.next_unit() * std::f32::consts::TAU;
            self.push(EffectParticle::square(
                position,
                Vec2::from_angle(angle).scale(speed),
                4.0,
                15,
                Color::from_rgba(224, 232, 240, 255),
                rotation,
            ));
        }
    }

    fn push(&mut self, particle: EffectParticle) {
        if self.particles.len() == MAX_PARTICLES {
            self.particles.remove(0);
        }
        self.particles.push(particle);
    }

    /// Advances an isolated fixed-width generator so effects never consume gameplay randomness.
    fn next_unit(&mut self) -> f32 {
        self.random_state ^= self.random_state << 7;
        self.random_state ^= self.random_state >> 9;
        self.random_state ^= self.random_state << 8;
        (self.random_state >> 40) as f32 / (1_u64 << 24) as f32
    }
}

#[derive(Clone, Copy, Debug)]
struct ImpactStyle {
    ring_size: f32,
    ring_weight: f32,
    ring_lifespan: u16,
    shard_count: usize,
    shard_size: f32,
    minimum_speed: f32,
    maximum_speed: f32,
    shard_lifespan: u16,
    color: Color,
}

#[derive(Clone, Copy, Debug)]
struct ShardStyle {
    count: usize,
    size: f32,
    minimum_speed: f32,
    maximum_speed: f32,
    lifespan: u16,
    color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ParticleShape {
    Square,
    Ring,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EffectParticle {
    position: Vec2,
    velocity: Vec2,
    shape: ParticleShape,
    rotation: f32,
    size: f32,
    stroke_weight: f32,
    age: u16,
    lifespan: u16,
    color: Color,
}

impl EffectParticle {
    fn square(
        position: Vec2,
        velocity: Vec2,
        size: f32,
        lifespan: u16,
        color: Color,
        rotation: f32,
    ) -> Self {
        Self {
            position,
            velocity,
            shape: ParticleShape::Square,
            rotation,
            size,
            stroke_weight: 1.0,
            age: 0,
            lifespan,
            color,
        }
    }

    fn ring(position: Vec2, size: f32, stroke_weight: f32, lifespan: u16, color: Color) -> Self {
        Self {
            position,
            velocity: Vec2::default(),
            shape: ParticleShape::Ring,
            rotation: 0.0,
            size,
            stroke_weight,
            age: 0,
            lifespan,
            color,
        }
    }

    fn advance(&mut self) {
        self.position = self.position.plus(self.velocity);
        self.velocity = self.velocity.scale(PARTICLE_FRICTION);
        if self.shape == ParticleShape::Square {
            self.rotation += PARTICLE_SQUARE_ROTATION_SPEED;
        }
        self.age = self.age.saturating_add(1);
    }

    fn is_alive(&self) -> bool {
        self.age <= self.lifespan
    }

    fn draw(&self, left: f32, top: f32, scale: f32) {
        let progress = f32::from(self.age) / f32::from(self.lifespan.max(1));
        let fade = (1.0 - progress).clamp(0.0, 1.0);
        let color = Color::new(
            self.color.r,
            self.color.g,
            self.color.b,
            self.color.a * fade,
        );
        let center = vec2(
            left + self.position.x * scale,
            top + self.position.y * scale,
        );

        match self.shape {
            ParticleShape::Square => {
                let size = self.size * scale;
                draw_rectangle_ex(
                    center.x,
                    center.y,
                    size,
                    size,
                    DrawRectangleParams {
                        offset: vec2(0.5, 0.5),
                        rotation: self.rotation,
                        color,
                    },
                );
            }
            ParticleShape::Ring => {
                let expansion = 2.0 * ((progress - 1.0).powi(5) + 1.0);
                let radius = self.size * (1.0 + expansion) * scale * 0.5;
                draw_circle_lines(
                    center.x,
                    center.y,
                    radius,
                    (self.stroke_weight * fade).max(0.5) * scale,
                    color,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dual_sim::{PlayerInput, Simulation, SimulationConfig};

    #[test]
    fn consuming_the_same_frame_twice_does_not_repeat_its_visual_events() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 17);
        simulation.step([
            PlayerInput {
                shortbow: true,
                ..PlayerInput::empty()
            },
            PlayerInput::empty(),
        ]);
        let snapshot = simulation.snapshot();
        let mut once = VisualEffects::default();
        once.advance(&snapshot);
        let mut repeated = once.clone();

        repeated.advance(&snapshot);

        assert_eq!(repeated, once);
        assert!(!once.particles.is_empty());
    }

    #[test]
    fn clearing_effects_discards_particles_and_accepts_the_next_round_clock() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 18);
        simulation.step([
            PlayerInput {
                shortbow: true,
                ..PlayerInput::empty()
            },
            PlayerInput::empty(),
        ]);
        let mut effects = VisualEffects::default();
        effects.advance(&simulation.snapshot());
        assert!(!effects.particles.is_empty());

        effects.clear();
        simulation.reset_round();
        simulation.step([PlayerInput::empty(); 2]);
        effects.advance(&simulation.snapshot());

        assert!(effects.particles.is_empty());
        assert_eq!(effects.last_frame, Some(1));
    }
}
