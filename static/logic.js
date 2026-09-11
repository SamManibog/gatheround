const area = document.getElementById('inputArea');

const threshold = 10; // Pixels moved to count as a drag instead of a tap

async function sendTrackpadDelta(x, y) {
    const response = await fetch('/input', {
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

    // Check if the response status is OK (200-299)
    if (!response.ok) {
        console.error(`ERROR [${response.status}]: ${await response.text()}`);
    }
}

async function sendTrackpadPress() {
    const response = await fetch('./input', {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        },
        body: JSON.stringify({
            kind: "TrackpadPress",
        })
    });

    // Check if the response status is OK (200-299)
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

area.addEventListener('pointerdown', (e) => {
    isTracking = true;
    startX = e.clientX;
    startY = e.clientY;
    area.setPointerCapture(e.pointerId);
});

area.addEventListener('pointermove', (e) => {
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

area.addEventListener('pointerup', (e) => {
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
