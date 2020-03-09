package dev.nhh.echo.server;

import dev.nhh.echo.server.network.ServerConnection;

public class Server {

    public static void main(String[] args) {
        ServerConnection server = new ServerConnection(4445);
        server.start();
        try {
            server.join();
        } catch (InterruptedException e) {
            e.printStackTrace();
        }
    }

}
