#include <Arduino.h>

// Diagnostic firmware: report physical Nano pins only. No pendant functions
// are assigned here, and no CNC/Mesa/HAL connection exists.
constexpr uint8_t FIRST_PIN = 2;
constexpr uint8_t LAST_PIN = 12;
constexpr uint8_t FIRST_SELECTOR_PIN = 4;
constexpr uint8_t LAST_SELECTOR_PIN = 11;
constexpr uint32_t BAUD_RATE = 115200;
constexpr uint32_t HEARTBEAT_MS = 1000;

volatile uint32_t d2Edges = 0;
volatile uint32_t d3Edges = 0;

void d2Changed() { ++d2Edges; }
void d3Changed() { ++d3Edges; }

uint16_t readPinMask() {
  uint16_t mask = 0;
  for (uint8_t pin = FIRST_PIN; pin <= LAST_PIN; ++pin) {
    if (digitalRead(pin) == HIGH) {
      mask |= static_cast<uint16_t>(1U << (pin - FIRST_PIN));
    }
  }
  return mask;
}

uint32_t readConnectionMask() {
  uint32_t connections = 0;
  uint8_t connectionBit = 0;

  for (uint8_t source = FIRST_SELECTOR_PIN;
       source <= LAST_SELECTOR_PIN; ++source) {
    for (uint8_t pin = FIRST_SELECTOR_PIN;
         pin <= LAST_SELECTOR_PIN; ++pin) {
      pinMode(pin, INPUT_PULLUP);
    }

    // Pull exactly one selector pin LOW; every other selector pin remains a
    // high-impedance input with its weak pull-up enabled.
    digitalWrite(source, LOW);
    pinMode(source, OUTPUT);
    delayMicroseconds(50);

    for (uint8_t target = source + 1;
         target <= LAST_SELECTOR_PIN; ++target) {
      if (digitalRead(target) == LOW) {
        connections |= (1UL << connectionBit);
      }
      ++connectionBit;
    }

    pinMode(source, INPUT_PULLUP);
  }

  return connections;
}

void printConnections(uint32_t connections) {
  Serial.print(F(",links="));
  bool printed = false;
  uint8_t connectionBit = 0;
  for (uint8_t first = FIRST_SELECTOR_PIN;
       first <= LAST_SELECTOR_PIN; ++first) {
    for (uint8_t second = first + 1;
         second <= LAST_SELECTOR_PIN; ++second) {
      if (connections & (1UL << connectionBit)) {
        if (printed) {
          Serial.print('+');
        }
        Serial.print('D');
        Serial.print(first);
        Serial.print(F("-D"));
        Serial.print(second);
        printed = true;
      }
      ++connectionBit;
    }
  }
  if (!printed) {
    Serial.print(F("none"));
  }
}

void printState(uint32_t sequence, uint32_t now, uint16_t mask,
                uint32_t connections, uint32_t edge2, uint32_t edge3) {
  Serial.print(F("R1,"));
  Serial.print(sequence);
  Serial.print(',');
  Serial.print(now);
  for (uint8_t pin = FIRST_PIN; pin <= LAST_PIN; ++pin) {
    Serial.print(F(",D"));
    Serial.print(pin);
    Serial.print('=');
    Serial.print((mask >> (pin - FIRST_PIN)) & 1U);
  }
  printConnections(connections);
  Serial.print(F(",D2edges="));
  Serial.print(edge2);
  Serial.print(F(",D3edges="));
  Serial.println(edge3);
}

void setup() {
  for (uint8_t pin = FIRST_PIN; pin <= LAST_PIN; ++pin) {
    pinMode(pin, INPUT_PULLUP);
  }

  attachInterrupt(digitalPinToInterrupt(2), d2Changed, CHANGE);
  attachInterrupt(digitalPinToInterrupt(3), d3Changed, CHANGE);

  Serial.begin(BAUD_RATE);
  delay(50);
  Serial.println(F("BOOT,R2,RAW_PINS_AND_D4_D11_CONNECTIONS,MONITOR_ONLY"));
}

void loop() {
  static uint16_t previousMask = 0xFFFF;
  static uint32_t previousConnections = 0xFFFFFFFFUL;
  static uint32_t previousEdge2 = 0;
  static uint32_t previousEdge3 = 0;
  static uint32_t lastReport = 0;
  static uint32_t sequence = 0;

  const uint32_t connections = readConnectionMask();
  const uint16_t mask = readPinMask();
  uint32_t edge2;
  uint32_t edge3;
  noInterrupts();
  edge2 = d2Edges;
  edge3 = d3Edges;
  interrupts();

  const uint32_t now = millis();
  if (mask != previousMask || connections != previousConnections ||
      edge2 != previousEdge2 ||
      edge3 != previousEdge3 || now - lastReport >= HEARTBEAT_MS) {
    printState(sequence++, now, mask, connections, edge2, edge3);
    previousMask = mask;
    previousConnections = connections;
    previousEdge2 = edge2;
    previousEdge3 = edge3;
    lastReport = now;
  }
}
