// -----------------------------------------------------------------------------------------------------------
// MAIN PAGE ELEMENTS
// -----------------------------------------------------------------------------------------------------------
function focusPage(page_name) {
    const pages = {
        joinsDisabledPage: document.getElementById('joinsDisabledPage'),
        joinPage: document.getElementById('joinPage'),
        inputPage: document.getElementById('inputPage'),
    };

    for (const page in pages) {
        pages[page].classList.add("hidden");
    }
    pages[page_name].classList.remove("hidden");
}

// -----------------------------------------------------------------------------------------------------------
// INPUT PAGE LOGIC
// -----------------------------------------------------------------------------------------------------------
const trackpadArea = document.getElementById('trackpad');

const threshold = 10; // Pixels moved to count as a drag instead of a tap

async function sendTrackpadDelta(x, y) {
    const response = await fetch('/games/player-api', {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            kind: "TrackpadDelta",
            delta_x: x,
            delta_y: y,
        })
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
}

async function sendTrackpadPress() {
    const response = await fetch('/games/player-api', {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            kind: "TrackpadPress",
        })
    });

    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
}

let startX = 0;
let startY = 0;
let lastX = 0;
let lastY = 0;
let isTracking = false;
let isDragging = false;

// trackpad sensitivity
let sensitivity = 1.2;

// minimum speed required for acceleration
let accelThreshold = 6.0;

// acceleration coefficient
let accelCoef = 0.7;

// acceleration exponent
let accelExp = 1.2;

trackpadArea.addEventListener('pointerdown', (e) => {
    isTracking = true;
    startX = e.clientX;
    startY = e.clientY;
    trackpadArea
        .setPointerCapture(e.pointerId);
});

trackpadArea.addEventListener('pointermove', (e) => {
    if (!isTracking) return;

    let currentX = e.clientX;
    let currentY = e.clientY;

    if (isDragging) {
        let diffX = currentX - lastX;
        let diffY = currentY - lastY;

        let speed = Math.sqrt(diffX * diffX + diffY * diffY);
        let acceleratable = Math.max(speed - accelThreshold, 0);

        let new_speed = speed + accelCoef * Math.pow(acceleratable, accelExp);
        let multiplier = new_speed / speed * sensitivity;

        sendTrackpadDelta(diffX * multiplier, diffY * multiplier);

        lastX = currentX;
        lastY = currentY;
    } else {
        let diffX = currentX - startX;
        let diffY = currentY - startY;

        // Check if movement exceeds threshold
        if (Math.abs(diffX) > threshold || Math.abs(diffY) > threshold) {
            isDragging = true;
            lastX = currentX;
            lastY = currentY;
        }
    }
});

trackpadArea.addEventListener('pointerup', (e) => {
    if (!isTracking) return;

    if (isDragging) {
        wasJustTapped = false;
    } else {
        wasJustTapped = true;
        sendTrackpadPress();
    }

    isTracking = false;
    isDragging = false;
});

let continuePlayerLoop = true;
async function initPlayerState() {
    const state_response = await fetch("/games/player-state", {
        method: "POST"
    })

    console.log("TODO: init player state, got: ", await state_response.text());

    // begin player event loop
    continueJoinLoop = false;
    continuePlayerLoop = true;
    handlePlayerEvents();
}

async function handlePlayerEvents() {
    const event_response = await fetch("/games/player-events", {
        method: "POST"
    })

    console.log("TODO: handle player events, got: ", await event_response.text());

    // call again to perpetuate loop
    if (continuePlayerLoop) {
        handlePlayerEvents();
    }
}

// -----------------------------------------------------------------------------------------------------------
// JOIN PAGE LOGIC
// -----------------------------------------------------------------------------------------------------------

// handle color button logic
let joinRequested = false;
const colorButtons = document.querySelectorAll('.color-selector-button');
async function handleColorButtonClick(event) {
    if (joinRequested) {
        return;
    }

    const color = event.target.value;
    console.log("Attempting join as ", color);

    joinRequested = true;
    const response = await fetch('/games/claim-player', {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            color: color,
        })
    });

    if (response.ok) {
        enterPlayerMode();
    } else {
        if (response.status == 423) {
            self.enterWaitMode();
        } else {
            console.error(`ERROR [${response.status}]: ${await response.text()}`);
        }
    }
    joinRequested = false;
}
colorButtons.forEach(button => {
    button.addEventListener('click', handleColorButtonClick);
});

let continueJoinLoop = true;
async function initJoinState() {
    const state_response = await fetch("/games/join-state", {
        method: "POST"
    })

    console.log("TODO: init join state, got: ", await state_response.text());

    // begin join event loop
    continuePlayerLoop = false;
    continueJoinLoop = true;
    handleJoinEvents();
}

async function handleJoinEvents() {
    const event_response = await fetch("/games/join-events", {
        method: "POST"
    })

    console.log("TODO: handle join events, got: ", await event_response.text());

    // call again to perpetuate loop
    if (continueJoinLoop) {
        handleJoinEvents();
    }
}

// -----------------------------------------------------------------------------------------------------------
// Reconnection logic
// -----------------------------------------------------------------------------------------------------------
async function enterPlayerMode() {
    console.log("entered player mode");
    focusPage("inputPage");
}

async function enterJoinMode() {
    console.log("entered join mode");
    focusPage("joinPage");
}

async function enterWaitMode() {
    console.log("entered wait mode");
    focusPage("joinsDisabledPage");
}

async function connect() {
    const response = await fetch('/games/join', {
        method: "POST",
    });

    const response_text = await response.text();

    if (response_text == "is player") {
        enterPlayerMode();
    } else if (response.ok) {
        enterJoinMode();
    } else {
        enterWaitMode();
    }
}
connect()
