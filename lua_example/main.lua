set_background("#E8E3DE")

local board_length = 1120
local board_outline = 12
local tile_length = board_length / 8

create_texture("chess_board", {
    {
        type = "pattern",
        pattern = "solid",
        rect = {
            x = -0.5 * board_length - board_outline,
            y = -0.5 * board_length - board_outline,
            width = board_length + board_outline * 2.0,
            height = board_length + board_outline * 2.0,
        },
        pattern_color = "#AA785A",
    },
    {
        type = "pattern",
        pattern = "checkers",
        rect = {
            x = -0.5 * board_length,
            y = -0.5 * board_length,
            width = board_length,
            height = board_length,
        },
        pattern_color = "#ECE7E2",
        background_color = "#AA785A",
        weight = tile_length,
    },
})

create_texture("red_square", {
    type = "pattern",
    pattern = "solid",
    rect = {
        x = -40,
        y = -40,
        width = 80,
        height = 80
    },
    pattern_color = {
        r = 255,
        g = 0,
        b = 0,
    }
})

create_texture("blue_king", {
    {
        type = "pattern",
        pattern = "solid",
        rect = {
            x = -64,
            y = -64,
            width = 128,
            height = 128
        },
        pattern_color = {
            r = 0,
            g = 0,
            b = 255,
        }
    },
    {
        type = "textbox",
        rect = {
            x = -60,
            y = -60,
            width = 120,
            height = 120
        },
        text = "King",
        align = "centercenter",
        font = "arial.ttf",
    }
})

create_piece({
    name = "red_piece",
    transform = { x = 400, y = 400 },
    texture = "red_square",
    colliders = {
        {
            shape = {{
                {x=-40,y=-40,},
                {x=40,y=-40,},
                {x=-40,y=40,},
                {x=40,y=40,},
            }},
        },
        {
            shape = {{
                {x=-40,y=-40,},
                {x=40,y=-40,},
                {x=-40,y=40,},
                {x=40,y=40,},
            }},
            layer = "grab",
        },
    },
})

create_piece({
    name = "blue_piece",
    transform = { x = 200, y = 200 },
    texture = "blue_king",
    colliders = {
        {
            shape = {{
                {x=-60,y=-60,},
                {x=60,y=-60,},
                {x=-60,y=60,},
                {x=60,y=60,},
            }},
        },
        {
            shape = {{
                {x=-60,y=-60,},
                {x=60,y=-60,},
                {x=-60,y=60,},
                {x=60,y=60,},
            }},
            layer = "grab",
        },
    },
})

local board = create_piece({
    name = "board",
    texture = "chess_board",
    draw_layer = -400,
})
center_item(board)

local board_transform = get_transform(board)

local top_left_tile = {
    x = board_transform.x - board_length * 0.5 + tile_length * 0.5,
    y = board_transform.y - board_length * 0.5 + tile_length * 0.5,
}
local tile_snap_radius = tile_length

local rows = "87654321"
local cols = "abcdefgh"

for r = 1, #rows do
    for c = 1, #cols do
        local transform = {
            x = top_left_tile.x + tile_length * (c - 1),
            y = top_left_tile.y + tile_length * (r - 1),
        }

        local color = "black"
        if (r + c) % 2 == 0 then
            color = "white"
        end

        local rank = rows:sub(r, r)
        local file = cols:sub(c, c)

        create_pile({
            name = file .. rank,
            snap_radius = tile_snap_radius,
            transform = transform,
            tags = {
                "rank"..rank,
                "file"..file,
                color,
                "tile"
            },
        })
    end
end

-- Function called after a player moves 
-- The argument passed is the player that was moved
function playerMoved(player)
    local snaps = get_snap_piles(player)
    set_snap_target(player, snaps[1])
end

return function(inputs)
    for _, input in ipairs(inputs) do
        local player = input.player

        move_player(player, input.trackpad_delta)
        set_snap_target(player, get_snap_piles(player)[1])

        if input.trackpad_pressed then
            if is_holding(player) then
                local held_item = get_held_item(player)
                local overlaps = get_item_overlaps(held_item);
                for _, layer in ipairs(overlaps.overlapping_layers) do
                    if layer ~= "grab" then
                        return
                    end
                end
                set_held_item(player, nil)
            else
                local top_item = get_top_piece_on_point(get_player_position(player))
                set_held_item(player, top_item)
                set_snap_target(player, get_snap_piles(player)[1])
            end
        end
    end
end
