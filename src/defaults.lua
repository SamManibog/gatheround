-- Function used to confirm a player grabbing a piece.
-- The arguments passed are the player attempting the grab and the piece (not pile) directly under the player's cursor.
-- This function should use the set_held_item API function to give the player the piece, confirming the action.
function grabAttempted(player, piece)
    return true
end

-- Function used to confirm a player dropping a piece.
-- The arguments passed are the player attempting the drop and the item (piece or pile) held by the player.
-- This function should use the set_held_item API function to remove the piece from the player's hand, confirming the action
function dropAttempted(player, item)
    -- allow drop if "grab" is the only intersecting layer
    for _, layer in ipairs(overlaps.overlapping_layers) do
        if layer ~= "grab" then
            return false
        end
    end
    return true
end
