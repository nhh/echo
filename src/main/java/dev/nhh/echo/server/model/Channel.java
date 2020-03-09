package dev.nhh.echo.server.model;

import dev.nhh.echo.client.dto.VoicePacket;
import dev.nhh.echo.server.network.ClientConnection;

import java.util.ArrayList;
import java.util.UUID;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicBoolean;

public class Channel extends Thread {

    private final UUID channelId;
    private final ConcurrentLinkedQueue<ClientConnection> clients = new ConcurrentLinkedQueue<>();
    private final ConcurrentLinkedQueue<VoicePacket> packets = new ConcurrentLinkedQueue<>();
    private final AtomicBoolean isRunning = new AtomicBoolean(true);

    public Channel(UUID id) {
        this.channelId = id;
    }

    public UUID getChannelId() {
        return this.channelId;
    }

    public void broadcast(VoicePacket packet) {
        this.packets.add(packet);
    }

    public void addClient(ClientConnection client) {
        this.clients.remove(client);
        this.clients.add(client);
        client.setChannel(this);
    }

    public void removeClient(ClientConnection client) {
        this.clients.remove(client);
        client.setChannel(null);
    }

    @Override
    public void run() {

        while(this.isRunning.get()) {

            if(this.packets.isEmpty()) {
                try {
                    Thread.sleep(16);
                } catch (InterruptedException e) {
                    e.printStackTrace();
                }
                continue;
            }

            VoicePacket packet = this.packets.poll();
            this.packets.remove(packet);

            for(ClientConnection client: clients) {
                if ( !client.getUserId().equals(packet.getUserId()) ) continue; // We dont want to hear ourselves
                client.send(packet);
            }

        }

    }

}
