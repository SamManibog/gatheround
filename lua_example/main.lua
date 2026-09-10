set_background("#E8E3DE")

local board_length = 1000
local tile_length = board_length / 8

create_texture("chess_board", {
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
            x = -60,
            y = -60,
            width = 120,
            height = 120
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
local tile_snap_radius = tile_length / 2 * 1.414

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

