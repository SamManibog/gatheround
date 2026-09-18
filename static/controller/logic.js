function logToSite() {
    document.getElementById("console").classList.remove("hidden");

    const oglog = console.log
    console.log = function (...args) {
        const elem = document.createElement('p');
        elem.textContent = args.join();
        document.getElementById("console").appendChild(elem);
    };
    console.error = function (...args) {
        const elem = document.createElement('p');
        elem.textContent = args.join();
        document.getElementById("console").appendChild(elem);
    };
}
//logToSite();

const trackpad = document.getElementById("trackpad");

let playerSocket = null;
let continueJoinLoop = true;
let continuePlayerLoop = false;

class InputCallbackManager {
    events = {}

    constructor(element_id) {
        this.element = document.getElementById(element_id);
    }

    setCallback(event, callback) {
        let old_callback = this.events[event];
        if (old_callback != null) {
            this.element.removeEventListener(event, old_callback);
        }
        this.element.addEventListener(event, callback);
        this.events[event] = callback;
    }

    clearCallbacks() {
        for (const [event, callback] of this.events) {
            this.element.removeEventListener(event, callback);
        }
    }
}

function getCookie(name) {
    const cookies = document.cookie.split('; ');
    for (let i = 0; i < cookies.length; i++) {
        const [key, value] = cookies[i].split('=');
        if (key === name) {
            return decodeURIComponent(value); // Decodes special characters
        }
    }
    return null; // Return null if the cookie doesn't exist
}

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
function sendTrackpadDelta(x, y) {
    if (playerSocket == null) {
        return;
    }

    playerSocket.send(JSON.stringify({
        kind: "TrackpadDelta",
        delta_x: x,
        delta_y: y,
    }));
}

function sendTrackpadPress() {
    if (playerSocket == null) {
        return;
    }

    playerSocket.send(JSON.stringify({
        kind: "TrackpadPress",
    }));
}

let startX = 0;
let startY = 0;
let lastX = 0;
let lastY = 0;
let isTracking = false;
let isDragging = false;

// pixels moved to count as a drag instead of a tap
const threshold = 10;

// trackpad sensitivity
let sensitivity = 1.2;

// minimum speed required for acceleration
let accelThreshold = 6.0;

// acceleration coefficient
let accelCoef = 0.7;

// acceleration exponent
let accelExp = 1.2;

trackpad.addEventListener('pointerdown', (e) => {
    isTracking = true;
    startX = e.clientX;
    startY = e.clientY;
    trackpad
        .setPointerCapture(e.pointerId);
});

trackpad.addEventListener('pointermove', (e) => {
    if (!isTracking) return;

    let currentX = e.clientX;
    let currentY = e.clientY;

    // Check if movement exceeds threshold
    if (!isDragging && (Math.abs(currentX - startX) > threshold || Math.abs(currentY - startY) > threshold)) {
        isDragging = true;
        lastX = currentX;
        lastY = currentY;
    }

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
    }
});

trackpad.addEventListener('pointerup', (e) => {
    if (!isTracking) return;

    if (!isDragging) {
        sendTrackpadPress();
    }

    isTracking = false;
    isDragging = false;
});

async function initPlayerState() {
    continueJoinLoop = false;
    continuePlayerLoop = true;

    const state_response = await fetch("/games/player-state", {
        method: "POST",
        credentials: 'include',
    })

    console.log("TODO: init player state, got: ", await state_response.text());

    // begin player event loop
    continuePlayerLoop = true;
    handlePlayerEvents();
}

async function handlePlayerEvents() {
    const event_response = await fetch("/games/player-events", {
        method: "POST",
        credentials: 'include',
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
const colorButtons = document.querySelectorAll('.color-selector-button');
function handleColorButtonClick(event) {
    if (playerSocket != null) {
        return;
    }

    const color = event.target.value;
    console.log("Attempting join as ", color);
    enterPlayerMode(color);
}
colorButtons.forEach(button => {
    button.addEventListener('click', handleColorButtonClick);
});

async function initJoinState() {
    continuePlayerLoop = false;
    continueJoinLoop = true;

    const response = await fetch("/games/join-state", {
        method: "POST",
        credentials: 'include',
    })

    if (response.ok) {
        handleJoinState(await response.json());
    } else {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }

    // begin join event loop
    continueJoinLoop = true;
    handleJoinEvents();
}

async function handleJoinEvents() {
    const response = await fetch("/games/join-events", {
        method: "POST",
        credentials: 'include',
    })

    if (response.ok) {
        handleJoinState(await response.json());
    } else {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }

    // call again to perpetuate loop
    if (continueJoinLoop) {
        handleJoinEvents();
    }
}

function handleJoinState(state) {
    const playerButtons = {
        red: document.getElementById("playerRed"),
        orange: document.getElementById("playerOrange"),
        yellow: document.getElementById("playerYellow"),
        green: document.getElementById("playerGreen"),
        blue: document.getElementById("playerBlue"),
        purple: document.getElementById("playerPurple"),
        pink: document.getElementById("playerPink"),
        lime: document.getElementById("playerLime"),
        cyan: document.getElementById("playerCyan"),
        black: document.getElementById("playerBlack"),
        white: document.getElementById("playerWhite"),
        brown: document.getElementById("playerBrown"),
    };

    const playerSet = new Set();
    for (player of state.players) {
        console.log("playing: ", player);
        playerSet.add(player);
    }

    for (buttonName in playerButtons) {
        const button = playerButtons[buttonName];
        if (playerSet.has(buttonName)) {
            button.classList.add("invisible");
        } else {
            button.classList.remove("invisible");
        }
    }
}

// -----------------------------------------------------------------------------------------------------------
// Reconnection logic
// -----------------------------------------------------------------------------------------------------------

/*
 * Gets a websocket connection to the given url
 * resolves to null rather than throwing an error
 */
function connectWebSocket(url) {
    return new Promise((resolve, reject) => {
        const socket = new WebSocket(url);

        // Resolve the promise when connection is successfully established
        socket.addEventListener('open', () => {
            console.log('WebSocket connection opened successfully.');
            resolve(socket);
        }, { once: true }); // Automatically cleans up the event listener

        // Reject the promise if there is a connection error
        socket.addEventListener('error', (error) => {
            console.error('WebSocket connection error:', error);
            resolve(null);
        }, { once: true });
    });
}
/*
 * Attempts to enter player mode as the given color
 * color is provided as a string or nullish value
 * color should only be nullish if a player session was established
*/
async function enterPlayerMode(color_opt) {
    console.log("establishing websocket");

    const sessionId = getCookie("session_id");

    const protocol = window.location.protocol === 'https:' ? 'wss://' : 'ws://';
    const socketUrl = `${protocol}${window.location.host}/games/player-ws`;

    // initialize socket
    playerSocket = await connectWebSocket(socketUrl);
    if (playerSocket == null) {
        return enterJoinMode();
    }

    // check for connection acceptance or rejection
    const connectionPromise = new Promise((resolve) => {
        function handleMessage(event) {
            playerSocket.removeEventListener('message', handleMessage);
            resolve(event);
        }
        playerSocket.addEventListener('message', handleMessage);
    });
    playerSocket.send(JSON.stringify({
        token: sessionId,
        color: color_opt,
    }));
    const connectionResponse = await connectionPromise;
    console.log("got socket init response: ", connectionResponse);
    if (connectionResponse.data == "rejected") {
        return enterJoinMode();
    }

    // do player mode things
    console.log("entered player mode");
    focusPage("inputPage");
    initPlayerState();
}

async function enterJoinMode() {
    console.log("entered join mode");

    playerSocket = null;
    focusPage("joinPage");
    initJoinState();
}

async function connect() {
    const response = await fetch('/games/join', {
        method: "POST",
        credentials: 'include',
    });

    const response_text = await response.text();

    if (response_text == "is player") {
        enterPlayerMode(null);
    } else {
        enterJoinMode();
    }
}
connect()
