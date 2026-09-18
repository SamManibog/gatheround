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
    const response = await fetch("/games/host-state", {
        method: "POST",
        credentials: 'include',
    })

    if (response.ok) {
        handleHostState(await response.json());
    } else {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }

    continueHostLoop = true;
    handleHostEvents();
}

const kickPlayerButtons = {
    red: document.getElementById("kickPlayerRed"),
    orange: document.getElementById("kickPlayerOrange"),
    yellow: document.getElementById("kickPlayerYellow"),
    green: document.getElementById("kickPlayerGreen"),
    blue: document.getElementById("kickPlayerBlue"),
    purple: document.getElementById("kickPlayerPurple"),
    pink: document.getElementById("kickPlayerPink"),
    lime: document.getElementById("kickPlayerLime"),
    cyan: document.getElementById("kickPlayerCyan"),
    black: document.getElementById("kickPlayerBlack"),
    white: document.getElementById("kickPlayerWhite"),
    brown: document.getElementById("kickPlayerBrown"),
};
for (buttonName in kickPlayerButtons) {
    const color = buttonName;
    kickPlayerButtons[buttonName].addEventListener("click", async (e) => {
        const response = await fetch('/games/host-api', {
            method: "POST",
            headers: {
                "Content-Type": "application/json"
            },
            credentials: 'include',
            body: JSON.stringify({
                kick_player: color,
            })
        });

        if (!response.ok) {
            console.error(`ERROR [${response.status}]: ${await response.text()}`);
        }
    });
}
function handleHostState(state) {
    const playerSet = new Set();
    for (player of state.players) {
        playerSet.add(player);
    }

    for (buttonName in kickPlayerButtons) {
        const button = kickPlayerButtons[buttonName];
        button.disabled = !state.kicks_allowed || state.host == buttonName || !playerSet.has(buttonName);
    }
}

async function handleHostEvents() {
    const response = await fetch("/games/host-events", {
        method: "POST",
        credentials: 'include',
    })

    if (response.ok) {
        handleHostState(await response.json());
    } else {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }

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
