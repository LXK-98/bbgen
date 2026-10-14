"""What bbgen weighs, in one place: the costs wiring and placement trade off against each other.

Every cost is in holes of wire length, so they compare directly: a slant (SHAPE_COST[2], 12) is worth a
detour of about twelve holes, a flexible jumper (FLEX_COST, 10) a little less. Wiring picks each wire by
these; placement's estimate picks layouts to build by its own (cheap, before anything is wired); and a
built layout is scored by the last group, which is what decides between layouts in the end. A weight
that stands for the same thing in two stages is defined once and named for both, so tuning one can't
quietly leave the other behind.

The modules take these values when they're imported: change them here, not on the modules.
"""

# ---- wiring (autowire): one wire, or a route of them ----

# What a wire's shape is worth (see Circuit._shape for the levels): straight and clear, straight over
# holes in use, slanted, lying along another wire or part.
SHAPE_COST = {0: 0.0, 1: 2.0, 2: 12.0, 3: 20.0}
CROSS_COST = 12.0       # per part body a wire lying on the board crosses, before it's bent around
BEND_COST = 4.0         # a wire bent around a part, on top of its length
COVERED_COST = 20.0     # an end in a hole under a two-lead part's body
CROSS_WIRE_COST = 1.0   # per wire a wire lying on the board crosses: wires may cross, but a tangle is hard to trace
KEPT_COST = 6.0         # a wire on the board ending in a rail end kept for a supply's flying lead
RELAY_COST = 3.0        # each empty strip a route passes through: one more wire to build
FLEX_COST = 10.0        # a flexible jumper rather than a wire on the board: more than a short bend, less than a slant
FLEX_OVER_COST = 6.0    # per part a flexible jumper arches over: it hides the part's pins, and comes out first
RAIL_LINK_COST = 3.0    # a wire joining two rails at their ends, whatever its length: board setup, not circuit

# flying leads, to off-board pins
OFF_RAIL_COST = 4.0     # a lead into a part's strip rather than onto a rail
RAIL_END_COST = 0.5     # per rail hole between a lead's end and the rail's end on the connector's side
LEAD_OVER_COST = 5.0    # per part or wire a lead passes over: more than landing off the rail
LEAD_TALL_COST = 40.0   # per tall part (a can, an LED, a pot) it would pass over: only if there's no other way

# ---- placement's estimate (autoplace), for footprint parts, before anything is wired ----

GAP_COST = 8.0          # a rail's pin in a column with no rail holes: its drop will slant
RAIL_COST = 0.5         # per row between a rail's pin and the rail
CROWD_COST = 4.0        # per signal wire past what a column has room for
LEG_LINK_COST = 1.0     # per hole between the nets a two-lead part joins
OFFBOARD_COST = 0.3     # per column between an off-board pin's net and the board's edge
SPACE_COST = 4.0        # per column short of three between two footprint parts
WALL_COST = 12.0        # a signal net with pins either side of a part no wire can lie across: a jumper over it
HALF_WALL_COST = 3.0    # ... or across one half of the board only: round by the other half
KEEP_COST = 1.5         # per hole of keep-out on the board (a USB cable is best off its edge)
ROOMLESS_COST = 20.0    # a two-lead part between two signal nets with no free strip in reach
ROOM_COST = 2.0         # per free row short of four in the strip of a pin in a net (a wide part a row off
                        # centre gives its busy side more)

# ---- placing two-lead parts' legs ----

ROUND_COST = 25.0       # per round body (an LED, a can) it would touch
LEG_SLANT_COST = 20.0   # a two-lead part slanting: worse than lying straight and needing a wire more
SECOND_RAIL_COST = 5.0  # a leg on a net's second rail, which then needs linking to the first
JUNCTION_COST = 6.0     # a leg in an empty strip of its own, which then needs a wire to its net
KEPT_DROP_COST = 6.0    # a leg whose straight drop to its rail would end in a rail end kept for a supply's lead
CROWD_LEG_COST = 1.5    # per hole in use right next to a leg

# ---- a built layout's score: what decides between layouts ----

FLYING_WEIGHT = 0.3     # per hole of flying lead
RAIL_LINK_WEIGHT = RAIL_LINK_COST  # a wire joining two rails, as wiring weighs it
CROSS_WEIGHT = 2.0      # per pair of wires crossing: what makes a layout look tangled
KEEP_WEIGHT = KEEP_COST  # per hole of keep-out on the board, as the estimate weighs it
FLEX_WEIGHT = 12.0      # per flexible jumper: better than a warned-about wire (15 and up), worse than a tidy one
LABEL_COST = 8.0        # per pair of two-lead parts close enough for their labels to collide
LEG_OVER_COST = 30.0    # per part body a two-lead part lies over: worse than a jumper or two
