const body = document.querySelector('body');

// applies a 1-second cooldown to a button
function applyCooldown(element) {
    element.disabled = true;

    setTimeout(() => {
        element.disabled = false;
    }, 1000);
}

document.getElementById("openLobbyButton").addEventListener("click", async (e) => {
    applyCooldown(e.target);

    const response = await fetch('/games/host/start-lobby', {
        method: "POST",
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
})

document.getElementById("openGameSelectorButton").addEventListener("click", async (e) => {
    applyCooldown(e.target);
    const response = await fetch('/games/host/select-game', {
        method: "POST",
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
})

async function isHost() {
    const host_response = await fetch('/games/host/is-host', {
        method: "POST",
    });

    let host_text = await host_response.text();
    if (!host_response.ok) {
        console.error(`ERROR [${host_response.status}]: ${host_text}`);
    }
    if (host_text == "true") {
        return true;
    }

    const claim_response = await fetch('/games/host/claim', {
        method: "POST",
    });

    if (!claim_response.ok) {
        console.error(`ERROR [${claim_response.status}]: ${await claim_response.text()}`);
        return false;
    }
    return true;
}

isHost().then((successful_claim) => {
    if (successful_claim) {
        body.classList.remove("hidden")
    }
})
