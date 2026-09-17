const body = document.querySelector('body');

// applies a 1-second cooldown to a button
function applyCooldown(element) {
    element.disabled = true;

    setTimeout(() => {
        element.disabled = false;
    }, 1000);
}

const openLobbyButton = document.getElementById("openLobbyButton")
openLobbyButton.addEventListener("click", async (e) => {
    applyCooldown(openGameSelectorButton);
    applyCooldown(openLobbyButton);

    const response = await fetch('/games/host-api', {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        credentials: 'include',
        body: JSON.stringify({
            start_lobby: true,
        })
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
})

const openGameSelectorButton = document.getElementById("openGameSelectorButton")
openGameSelectorButton.addEventListener("click", async (e) => {
    applyCooldown(openGameSelectorButton);
    applyCooldown(openLobbyButton);

    const response = await fetch('/games/host-api', {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        credentials: 'include',
        body: JSON.stringify({
            start_game_selector: true
        })
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
})

let continueHostLoop = true;
async function initHostState() {
    const state_response = await fetch("/games/host-state", {
        method: "POST",
        credentials: 'include',
    })

    console.log("TODO: init state, got: ", await state_response.text());

    continueHostLoop = true;
    handleHostEvents();
}

async function handleHostEvents() {
    const event_response = await fetch("/games/host-events", {
        method: "POST",
        credentials: 'include',
    })

    console.log("TODO: handle events, got: ", await event_response.text());

    // call again to perpetuate loop
    if (continueHostLoop) {
        handleHostEvents();
    }
}

async function claimHost() {
    const claim_response = await fetch('/games/claim-host', {
        method: "POST",
        credentials: 'include',
    });

    if (!claim_response.ok) {
        console.error(`ERROR [${claim_response.status}]: ${await claim_response.text()}`);
        return false;
    }
    return true;
}

claimHost().then(async (successful_claim) => {
    if (successful_claim) {
        body.classList.remove("hidden");
        initHostState();
    }
})
