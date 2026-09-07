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
    --[[{
        type = "pattern",
        pattern = "dots",
        rect = {
            x = -40,
            y = -40,
            width = 80,
            height = 80
        },
        pattern_color = {
            r = 0,
            g = 0,
            b = 255,
        },
        background_color = {
            r = 0,
            g = 255,
            b = 255,
        },
        offset = {
            x = 55,
            y = 0,
        },
        weight = 20,
        spacing = 5,
        angle = 0,
    },]]
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
