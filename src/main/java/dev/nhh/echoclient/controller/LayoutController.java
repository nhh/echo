package dev.nhh.echoclient.controller;

import dev.nhh.echoclient.audio.speaker.SpeakerThreadWrapper;
import dev.nhh.echoclient.client.ClientThreadWrapper;
import dev.nhh.echoclient.util.ThreadScheduler;
import javafx.fxml.FXML;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;
import javafx.stage.Window;

public class LayoutController {

    @FXML
    private TextField textField;

    @FXML
    private Window dialog;

    @FXML
    protected void startMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new ClientThreadWrapper("localhost"));
        ThreadScheduler.INSTANCE.start("clientThreadWrapper");
    }

    @FXML
    protected void stopMicrophone(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("clientThreadWrapper");
    }

    @FXML
    protected void start(MouseEvent event) {
        ThreadScheduler.INSTANCE.addWrapper(new SpeakerThreadWrapper());
        ThreadScheduler.INSTANCE.start("speakerThreadWrapper");
    }

    @FXML
    protected void stop(MouseEvent event) {
        ThreadScheduler.INSTANCE.stop("speakerThreadWrapper");
    }

}
