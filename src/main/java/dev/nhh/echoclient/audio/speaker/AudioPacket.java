package dev.nhh.echoclient.audio.speaker;

public class AudioPacket {

    private final byte[] bytes;
    private final int offset;
    private final int length;

    public AudioPacket(byte[] bytes, int offset, int length) {
        this.bytes = bytes;
        this.offset = offset;
        this.length = length;
    }

    public byte[] getBytes() {
        return bytes;
    }

    public int getOffset() {
        return offset;
    }

    public int getLength() {
        return length;
    }

}
