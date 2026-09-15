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

    const response = await fetch('/gamehost/start-lobby', {
        method: "POST",
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
})

document.getElementById("openGameSelectorButton").addEventListener("click", async (e) => {
    applyCooldown(e.target);
    const response = await fetch('/gamehost/select-game', {
        method: "POST",
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
})

async function claimHost() {
    console.log("claiming");
    const response = await fetch('/gamehost/claim', {
        method: "POST",
    });
    console.log("claimed");

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
        return false;
    }
    return true;
}

claimHost().then((successful_claim) => {
    if (successful_claim) {
        body.classList.remove("hidden")
    }
})
