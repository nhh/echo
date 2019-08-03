package dev.nhh.echo.server.network;

import dev.nhh.echo.client.dto.HandshakePacket;
import dev.nhh.echo.client.dto.VoicePacket;
import dev.nhh.echo.server.model.Channel;

import java.io.IOException;
import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.net.Socket;
import java.util.ArrayList;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class ClientConnection extends Thread {

    private Channel channel; // Server Channel
    private UUID id;

    private ObjectInputStream in; // Stream from client (receive)
    private ObjectOutputStream out; // Stream to client (send)

    private final ArrayList<VoicePacket> queue = new ArrayList<>();
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    public ClientConnection(Socket socket) {
        try {
            socket.setTcpNoDelay(true);
            socket.setKeepAlive(true);
            this.out = new ObjectOutputStream(socket.getOutputStream()); //create object streams to/from client
            this.in = new ObjectInputStream(socket.getInputStream());
        } catch (IOException e) {
            e.printStackTrace();
        }
    }

    public void send(VoicePacket packet) {
        this.queue.add(packet);
    }

    @Override
    public void run() {
        if (this.isRunning.get()) {
            return;
        }

        this.isRunning.set(true);

        while(isRunning.get()) {

            while(this.id == null) {
                receiveHandshake();
            }

            receivePackage();
            sendPackage();
        }

        this.channel.removeClient(this);

    }

    private void receiveHandshake() {
        try {

            HandshakePacket hp = (HandshakePacket) in.readObject();

            if(hp == null || hp.getUserId() == null) {
                return;
            }

            this.id = hp.getUserId();
            System.out.println("Received handshake from " + hp.getUserId());
        } catch (IOException | ClassNotFoundException e) {
            e.printStackTrace();
        }
    }

    private void receivePackage() {
        try {

            VoicePacket vp = (VoicePacket) in.readObject();

            if(vp.getChannelId() != null) {
                return;
            }

            if (this.channel == null) {
                return;
            }

            vp.setTimestamp(System.nanoTime() / 1000000L);
            vp.setChannelId(this.channel.getChannelId());
            this.channel.broadcast(vp); // Send packet to all clients within channel

        } catch (IOException | ClassNotFoundException e) {
            this.isRunning.set(false);
        }
    }

    private void sendPackage() {
        try {
            if (queue.isEmpty()) {
                return;
            }

            VoicePacket packet = queue.get(0); // we got something to send to the client
            queue.remove(0);

            if (packet == null || packet.getUserId() == null || !packet.isAlive()) { //is the message too old or of an unknown type?
                System.out.println("dropping packet from " + packet.getUserId() + " to " + packet.getChannelId());
            }

            out.writeObject(packet); //send the message

        } catch(IOException e) {
            e.printStackTrace();
        }
    }

    public void setChannel(Channel channel) {
        this.channel = channel;
    }

    public UUID getUserId() {
        return this.id;
    }
}
