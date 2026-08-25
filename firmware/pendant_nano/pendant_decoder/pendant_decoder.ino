#include <Arduino.h>

// USER-OBSERVED ACTUAL Nano wiring, confirmed by live signal mapping.
constexpr uint8_t PIN_ENCODER_A = 2;  // green
constexpr uint8_t PIN_ENCODER_B = 3;  // white

constexpr uint8_t AXIS_PINS[] = {4, 5, 6, 7, 8};
constexpr char AXIS_NAMES[] = {'X', 'Y', 'Z', '4', '5'};
constexpr uint8_t MULTIPLIER_PINS[] = {9, 10, 11};
constexpr uint16_t MULTIPLIER_VALUES[] = {1, 10, 100};
constexpr uint8_t PIN_ESTOP = 12;

constexpr uint32_t BAUD_RATE = 115200;
constexpr uint32_t REPORT_INTERVAL_MS = 20;
constexpr uint32_t SELECTOR_DEBOUNCE_MS = 20;
constexpr uint16_t SELECTOR_SETTLE_US = 50;

constexpr uint8_t SELECT_NONE = 0;
constexpr uint8_t SELECT_INVALID = 0xFF;

struct SelectorState {
  uint8_t axis;
  uint8_t multiplier;
  bool deadmanHeld;
};

volatile int32_t transitionCount = 0;
volatile int32_t detentCount = 0;
volatile int8_t partialDetent = 0;
// One-slot, latest-wins command sample. Every report consumes and clears it.
volatile int8_t latestDetentSignal = 0;
volatile uint32_t quadratureErrors = 0;
volatile uint8_t previousAB = 0;

// Index is previous AB in bits 3:2 and current AB in bits 1:0. The measured
// clockwise sequence 00 -> 10 -> 11 -> 01 -> 00 is positive.
constexpr int8_t QUADRATURE_LUT[16] = {
    0, -1, +1, 0,
   +1,  0,  0, -1,
   -1,  0,  0, +1,
    0, +1, -1, 0,
};

SelectorState selectorCandidate = {SELECT_NONE, SELECT_NONE, false};
SelectorState selectorStable = {SELECT_NONE, SELECT_NONE, false};
uint32_t selectorCandidateSince = 0;
bool selectorSettling = true;
bool previousEstopPressed = true;
uint32_t reportSequence = 0;
uint32_t lastReportAt = 0;

inline uint8_t readAB() {
  return static_cast<uint8_t>((digitalRead(PIN_ENCODER_A) << 1) |
                              digitalRead(PIN_ENCODER_B));
}

void encoderChanged() {
  const uint8_t currentAB = readAB();
  if ((previousAB ^ currentAB) == 0x03) {
    ++quadratureErrors;
  }

  const int8_t delta = QUADRATURE_LUT[(previousAB << 2) | currentAB];
  transitionCount += delta;
  partialDetent += delta;
  if (partialDetent >= 4) {
    ++detentCount;
    partialDetent -= 4;
    latestDetentSignal = +1;
  } else if (partialDetent <= -4) {
    --detentCount;
    partialDetent += 4;
    latestDetentSignal = -1;
  }
  previousAB = currentAB;
}

uint8_t readSingleLow(const uint8_t *pins, uint8_t count) {
  uint8_t selected = SELECT_NONE;
  for (uint8_t index = 0; index < count; ++index) {
    if (digitalRead(pins[index]) == LOW) {
      if (selected != SELECT_NONE) {
        return SELECT_INVALID;
      }
      selected = static_cast<uint8_t>(index + 1);
    }
  }
  return selected;
}

void restoreSelectorInputs() {
  for (uint8_t pin : AXIS_PINS) {
    pinMode(pin, INPUT_PULLUP);
  }
  for (uint8_t pin : MULTIPLIER_PINS) {
    pinMode(pin, INPUT_PULLUP);
  }
}

SelectorState readSelectorState() {
  restoreSelectorInputs();

  // Holding the side button grounds exactly the selected axis and multiplier.
  const uint8_t groundedAxis =
      readSingleLow(AXIS_PINS, sizeof(AXIS_PINS));
  const uint8_t groundedMultiplier =
      readSingleLow(MULTIPLIER_PINS, sizeof(MULTIPLIER_PINS));

  if (groundedAxis == SELECT_INVALID ||
      groundedMultiplier == SELECT_INVALID) {
    return {SELECT_INVALID, SELECT_INVALID, false};
  }
  if (groundedAxis != SELECT_NONE || groundedMultiplier != SELECT_NONE) {
    // A held side button grounds every selector line that currently has a
    // physical selection. Preserve a partial selection so recovery can see
    // multiplier x1 while the axis selector is physically OFF.
    return {groundedAxis, groundedMultiplier, true};
  }

  // With the side button released, the selected axis and multiplier form a
  // dry-contact pair. Drive one multiplier LOW at a time and read the axes.
  SelectorState found = {SELECT_NONE, SELECT_NONE, false};
  for (uint8_t multiplier = 0;
       multiplier < sizeof(MULTIPLIER_PINS); ++multiplier) {
    restoreSelectorInputs();
    const uint8_t sourcePin = MULTIPLIER_PINS[multiplier];
    digitalWrite(sourcePin, LOW);
    pinMode(sourcePin, OUTPUT);
    delayMicroseconds(SELECTOR_SETTLE_US);

    const uint8_t axis = readSingleLow(AXIS_PINS, sizeof(AXIS_PINS));
    pinMode(sourcePin, INPUT_PULLUP);

    if (axis == SELECT_INVALID) {
      return {SELECT_INVALID, SELECT_INVALID, false};
    }
    if (axis != SELECT_NONE) {
      if (found.axis != SELECT_NONE) {
        return {SELECT_INVALID, SELECT_INVALID, false};
      }
      found.axis = axis;
      found.multiplier = static_cast<uint8_t>(multiplier + 1);
    }
  }
  restoreSelectorInputs();
  return found;
}

bool selectorEqual(const SelectorState &left, const SelectorState &right) {
  return left.axis == right.axis &&
         left.multiplier == right.multiplier &&
         left.deadmanHeld == right.deadmanHeld;
}

void discardPendingWheelMotion() {
  noInterrupts();
  partialDetent = 0;
  latestDetentSignal = 0;
  previousAB = readAB();
  interrupts();
}

void updateDebouncedSelector(const SelectorState &raw, uint32_t now) {
  if (!selectorEqual(raw, selectorCandidate)) {
    selectorCandidate = raw;
    selectorCandidateSince = now;
    selectorSettling = true;
    // A selector or side-button transition invalidates every wheel fragment
    // observed under the preceding physical state.
    discardPendingWheelMotion();
    return;
  }
  if (selectorSettling &&
      now - selectorCandidateSince >= SELECTOR_DEBOUNCE_MS) {
    selectorStable = selectorCandidate;
    selectorSettling = false;
    // Do not let a partial turn made during the debounce interval become the
    // first command under the newly accepted selection.
    discardPendingWheelMotion();
  }
}

SelectorState publishedSelectorState() {
  if (selectorSettling) {
    return {SELECT_INVALID, SELECT_INVALID, false};
  }
  return selectorStable;
}

void printAxis(uint8_t axis) {
  if (axis >= 1 && axis <= sizeof(AXIS_NAMES)) {
    Serial.print(AXIS_NAMES[axis - 1]);
  } else if (axis == SELECT_NONE) {
    Serial.print('N');
  } else {
    Serial.print('I');
  }
}

void printMultiplier(uint8_t multiplier) {
  if (multiplier >= 1 && multiplier <= 3) {
    Serial.print('X');
    Serial.print(MULTIPLIER_VALUES[multiplier - 1]);
  } else if (multiplier == SELECT_NONE) {
    Serial.print('N');
  } else {
    Serial.print('I');
  }
}

void emitStatus(uint32_t now) {
  const bool estopPressed = digitalRead(PIN_ESTOP) == HIGH;
  if (estopPressed != previousEstopPressed) {
    previousEstopPressed = estopPressed;
    // A wheel fragment may never cross either edge of the E-stop signal.
    discardPendingWheelMotion();
  }

  int32_t detentSnapshot;
  int32_t transitionSnapshot;
  int8_t latestDetentSnapshot;
  uint32_t errorSnapshot;
  noInterrupts();
  detentSnapshot = detentCount;
  transitionSnapshot = transitionCount;
  latestDetentSnapshot = latestDetentSignal;
  latestDetentSignal = 0;
  errorSnapshot = quadratureErrors;
  interrupts();

  const SelectorState publishedSelector = publishedSelectorState();
  const bool selectorValid =
      publishedSelector.axis != SELECT_NONE &&
      publishedSelector.axis != SELECT_INVALID &&
      publishedSelector.multiplier != SELECT_NONE &&
      publishedSelector.multiplier != SELECT_INVALID;

  Serial.print(F("P3,"));
  Serial.print(reportSequence++);
  Serial.print(',');
  Serial.print(now);
  Serial.print(',');
  Serial.print(detentSnapshot);
  Serial.print(',');
  Serial.print(transitionSnapshot);
  Serial.print(',');
  Serial.print(errorSnapshot);
  Serial.print(',');
  Serial.print(latestDetentSnapshot);
  Serial.print(',');
  printAxis(publishedSelector.axis);
  Serial.print(',');
  printMultiplier(publishedSelector.multiplier);
  Serial.print(',');
  Serial.print(publishedSelector.deadmanHeld ? 1 : 0);
  Serial.print(',');
  Serial.print(estopPressed ? 1 : 0);
  Serial.print(',');
  Serial.println(selectorValid ? 1 : 0);
}

void setup() {
  pinMode(PIN_ENCODER_A, INPUT);
  pinMode(PIN_ENCODER_B, INPUT);
  restoreSelectorInputs();
  pinMode(PIN_ESTOP, INPUT_PULLUP);

  previousAB = readAB();
  const uint32_t now = millis();
  selectorCandidate = readSelectorState();
  selectorStable = {SELECT_INVALID, SELECT_INVALID, false};
  selectorCandidateSince = now;
  selectorSettling = true;
  previousEstopPressed = digitalRead(PIN_ESTOP) == HIGH;

  attachInterrupt(digitalPinToInterrupt(PIN_ENCODER_A), encoderChanged, CHANGE);
  attachInterrupt(digitalPinToInterrupt(PIN_ENCODER_B), encoderChanged, CHANGE);

  Serial.begin(BAUD_RATE);
  delay(50);
  Serial.println(F("BOOT,P3,MYST1474-001,MONITOR_ONLY"));
  lastReportAt = millis();
}

void loop() {
  const uint32_t now = millis();
  updateDebouncedSelector(readSelectorState(), now);

  if (now - lastReportAt >= REPORT_INTERVAL_MS) {
    lastReportAt = now;
    emitStatus(now);
  }
}
