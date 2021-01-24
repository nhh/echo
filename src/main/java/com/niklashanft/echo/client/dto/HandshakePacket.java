package com.niklashanft.echo.client.dto;


import java.io.Serializable;
import java.util.UUID;

// Todo refactor
public class HandshakePacket implements Serializable {

    private UUID userId; // Which audioline the client should assign this packet
    private long timestamp, ttl=2000;

    public HandshakePacket(UUID userId, long timestamp) {
        this.userId = userId;
        this.timestamp = timestamp;
    }

    public boolean isAlive() {
        return this.timestamp + this.ttl > System.nanoTime() / 1000000L;
    }

    public UUID getUserId() {
        return userId;
    }

}
