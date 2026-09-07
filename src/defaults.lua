-- function used to confirm a player grabbing a piece
function allowGrab(player, piece)
    return true
end

-- function used to confirm a player dropping a piece
function allowDrop(player, piece, overlaps)
    -- allow drop if there are no intersecting pieces
    if #overlaps == 0 then
        return true
    end

    -- allow drop if the only intersection is with the grab layer
    if overlaps["grab"] ~= nil and #overlaps == 1 then
        return true
    end
    return false
end
