package dev.nhh.echo.server.network;

import dev.nhh.echo.server.util.ClientList;

import java.io.IOException;
import java.net.*;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class ServerConnection extends Thread {

    private AtomicBoolean isRunning = new AtomicBoolean(false);
    private ServerSocket socket;

    public ServerConnection(int port) {
        try {
            this.socket = new ServerSocket(port);
        } catch (IOException e) {
            e.printStackTrace();
        }
    }

    @Override
    public void run() {
        if(this.isRunning.get()) {
            return;
        }

        this.isRunning.set(true);

        while (isRunning.get()) {

            try {
                Socket c = socket.accept();
                System.out.println("Accepting client" + c.getInetAddress());
                var connection = new ClientConnection(UUID.randomUUID(), c);
                connection.start();
                ClientList.INSTANCE.add(connection);
            } catch (IOException e) {
                e.printStackTrace();
            }
        }

    }

}

//
