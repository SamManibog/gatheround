const body = document.querySelector('body');

function focusPage(page_name) {
    const pages = {
        joinsDisabledPage: document.getElementById('joinsDisabledPage'),
        dashboardPage: document.getElementById("dashboardPage"),
        hostCandidatePage: document.getElementById("hostCandidatePage")
    };

    for (const page in pages) {
        pages[page].classList.add("hidden");
    }
    pages[page_name].classList.remove("hidden");
}

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
    focusPage("dashboardPage")

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

// run setup steps
(async () => {})().then(async () => {
    const session_response = await fetch('/games/join', {
        method: "POST",
        credentials: 'include',
    });

    if (!session_response.ok) {
        focusPage("joinsDisabledPage");
    } else {
        const session_state = await session_response.json();
        if (session_state.is_host) {
            return initHostState();
        }

        const host_code_response = await fetch('claim-host', {
            method: "POST",
            credentials: 'include',
        });

        if (!host_code_response.ok) {
            console.log(await host_code_response.text());
            return focusPage("joinsDisabledPage");
        }

        document.getElementById("hostCode").textContent = await host_code_response.text();
        focusPage("hostCandidatePage");

        const is_host_state = await fetch("/games/is-host", {
            method: "POST",
            credentials: 'include',
        })

        if (is_host_state.ok) {
            initHostState();
        } else {
            focusPage("joinsDisabledPage");
        }
    }
})
