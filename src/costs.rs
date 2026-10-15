//! What bbgen weighs, in one place: the costs wiring and placement trade off against each other.
//!
//! Every cost is in holes of wire length, so they compare directly: a slant (SHAPE_COST[2], 12) is worth a
//! detour of about twelve holes, a flexible jumper (FLEX_COST, 10) a little less. Wiring picks each wire by
//! these; placement's estimate picks layouts to build by its own (cheap, before anything is wired); and a
//! built layout is scored by the last group, which is what decides between layouts in the end. A weight
//! that stands for the same thing in two stages is defined once and named for both, so tuning one can't
//! quietly leave the other behind.
//!
//! Change them here: every stage takes them from this file.

// ---- wiring (autowire): one wire, or a route of them ----

/// What a wire's shape is worth (see Circuit::shape for the levels): straight and clear, straight over
/// holes in use, slanted, lying along another wire or part.
pub const SHAPE_COST: [f64; 4] = [0.0, 2.0, 12.0, 20.0];
/// per part body a wire lying on the board crosses, before it's bent around
pub const CROSS_COST: f64 = 12.0;
/// a wire bent around a part, on top of its length
pub const BEND_COST: f64 = 4.0;
/// an end in a hole under a two-lead part's body
pub const COVERED_COST: f64 = 20.0;
/// per wire a wire lying on the board crosses: wires may cross, but a tangle is hard to trace
pub const CROSS_WIRE_COST: f64 = 1.0;
/// a wire on the board ending in a rail end kept for a supply's flying lead
pub const KEPT_COST: f64 = 6.0;
/// each empty strip a route passes through: one more wire to build
pub const RELAY_COST: f64 = 3.0;
/// a flexible jumper rather than a wire on the board: more than a short bend, less than a slant
pub const FLEX_COST: f64 = 10.0;
/// per part a flexible jumper arches over: it hides the part's pins, and comes out first
pub const FLEX_OVER_COST: f64 = 6.0;
/// a wire joining two rails at their ends, whatever its length: board setup, not circuit
pub const RAIL_LINK_COST: f64 = 3.0;

// flying leads, to off-board pins
/// a lead into a part's strip rather than onto a rail
pub const OFF_RAIL_COST: f64 = 4.0;
/// per rail hole between a lead's end and the rail's end on the connector's side
pub const RAIL_END_COST: f64 = 0.5;
/// per part or wire a lead passes over: more than landing off the rail
pub const LEAD_OVER_COST: f64 = 5.0;
/// per tall part (a can, an LED, a pot) it would pass over: only if there's no other way
pub const LEAD_TALL_COST: f64 = 40.0;

// ---- placement's estimate (autoplace), for footprint parts, before anything is wired ----

/// a rail's pin in a column with no rail holes: its drop will slant
pub const GAP_COST: f64 = 8.0;
/// per row between a rail's pin and the rail
pub const RAIL_COST: f64 = 0.5;
/// per signal wire past what a column has room for
pub const CROWD_COST: f64 = 4.0;
/// per hole between the nets a two-lead part joins
pub const LEG_LINK_COST: f64 = 1.0;
/// per column between an off-board pin's net and the board's edge
pub const OFFBOARD_COST: f64 = 0.3;
/// per column short of three between two footprint parts
pub const SPACE_COST: f64 = 4.0;
/// a signal net with pins either side of a part no wire can lie across: a jumper over it
pub const WALL_COST: f64 = 12.0;
/// ... or across one half of the board only: round by the other half
pub const HALF_WALL_COST: f64 = 3.0;
/// per hole of keep-out on the board (a USB cable is best off its edge)
pub const KEEP_COST: f64 = 1.5;
/// a two-lead part between two signal nets with no free strip in reach
pub const ROOMLESS_COST: f64 = 20.0;
/// per free row short of four in the strip of a pin in a net (a wide part a row off centre gives its busy side more)
pub const ROOM_COST: f64 = 2.0;

// ---- placing two-lead parts' legs ----

/// per round body (an LED, a can) it would touch
pub const ROUND_COST: f64 = 25.0;
/// a two-lead part slanting: worse than lying straight and needing a wire more
pub const LEG_SLANT_COST: f64 = 20.0;
/// a leg on a net's second rail, which then needs linking to the first
pub const SECOND_RAIL_COST: f64 = 5.0;
/// a leg in an empty strip of its own, which then needs a wire to its net
pub const JUNCTION_COST: f64 = 6.0;
/// a leg whose straight drop to its rail would end in a rail end kept for a supply's lead
pub const KEPT_DROP_COST: f64 = 6.0;
/// per hole in use right next to a leg
pub const CROWD_LEG_COST: f64 = 1.5;

// ---- a built layout's score: what decides between layouts ----

/// per hole of flying lead
pub const FLYING_WEIGHT: f64 = 0.3;
/// a wire joining two rails, as wiring weighs it
pub const RAIL_LINK_WEIGHT: f64 = RAIL_LINK_COST;
/// per pair of wires crossing: what makes a layout look tangled
pub const CROSS_WEIGHT: f64 = 2.0;
/// per hole of keep-out on the board, as the estimate weighs it
pub const KEEP_WEIGHT: f64 = KEEP_COST;
/// per flexible jumper: better than a warned-about wire (15 and up), worse than a tidy one
pub const FLEX_WEIGHT: f64 = 12.0;
/// per pair of two-lead parts close enough for their labels to collide
pub const LABEL_COST: f64 = 8.0;
/// per part body a two-lead part lies over: worse than a jumper or two
pub const LEG_OVER_COST: f64 = 30.0;
