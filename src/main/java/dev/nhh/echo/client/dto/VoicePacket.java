package dev.nhh.echo.client.dto;


import java.io.Serializable;
import java.util.UUID;

public class VoicePacket implements Serializable {

    private UUID channelId; // Which channel should be broadcast to?
    private UUID userId; // Which audioline the client should assign this packet

    private long timestamp, ttl=2000;

    private byte[] data; //can carry any type of object. in this program, i used a sound packet, but it could be a string, a chunk of video, ...


    public VoicePacket(UUID channelId, UUID userId, long timestamp, byte[] data) {
        this.userId = userId;
        this.channelId = channelId;
        this.timestamp = timestamp;
        this.data = data;
    }

    public void setTimestamp(long timestamp) {
        this.timestamp = timestamp;
    }

    public UUID getChannelId() {
        return channelId;
    }

    public byte[] getData() {
        return data;
    }

    public long getTimestamp() {
        return timestamp;
    }

    public long getTtl() {
        return ttl;
    }
    public void setTtl(long ttl) {
        this.ttl = ttl;
    }

    public void setChannelId(UUID chId) {
        this.channelId = chId;
    }

    public UUID getUserId() {
        return userId;
    }
    public void setUserId(UUID id) { this.userId = id; }

}
