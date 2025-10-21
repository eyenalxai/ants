use crate::components::{Ant, Food, Nest};
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn check_collisions(
    mut ant_query: Query<(&mut Ant, &Transform)>,
    food_query: Query<&Transform, With<Food>>,
    nest_query: Query<&Transform, With<Nest>>,
) {
    let Some(food_transform) = food_query.iter().next() else {
        return;
    };
    let Some(nest_transform) = nest_query.iter().next() else {
        return;
    };
    let food_pos = Vec2::new(food_transform.translation.x, food_transform.translation.y);
    let nest_pos = Vec2::new(nest_transform.translation.x, nest_transform.translation.y);
    let food_radius = 15.0 / 2.0;
    let nest_radius = 40.0 / 2.0;

    for (mut ant, transform) in &mut ant_query {
        let ant_pos = Vec2::new(transform.translation.x, transform.translation.y);

        if !ant.has_food && ant_pos.distance(food_pos) < food_radius {
            ant.has_food = true;
            ant.direction = (ant.direction + PI).rem_euclid(2.0 * PI);
        } else if ant.has_food && ant_pos.distance(nest_pos) < nest_radius {
            ant.has_food = false;
            ant.direction = (ant.direction + PI).rem_euclid(2.0 * PI);
        }
    }
}
