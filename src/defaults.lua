-- Function used to confirm a player grabbing a piece.
-- The arguments passed are the player attempting the grab and the piece (not pile) directly under the player's cursor.
-- This function should use the set_held_item API function to give the player the piece, confirming the action.
function grabAttempted(player)
    if not is_holding(player) then
        local top_item = get_top_piece_on_point(get_player_position(player))
        set_held_item(player, top_item)
    end
end

-- Function used to confirm a player dropping a piece.
-- The arguments passed is the player attempting the drop
-- This function should use the set_held_item API function to remove the piece from the player's hand, confirming the action
function dropAttempted(player)
    local held_item = get_held_item(player)
    if held_item == nil then
        return
    end
    local overlaps = get_item_overlaps(held_item);

    -- allow drop if "grab" is the only intersecting layer
    for _, layer in ipairs(overlaps.overlapping_layers) do
        if layer ~= "grab" then
            return
        end
    end

    set_held_item(player, nil)
end

-- Function called after a player moves 
-- The argument passed is the player that was moved
function playerMoved(player)
    local snaps = get_snap_piles(player)

    set_snap_target(player, snaps[1])
end
