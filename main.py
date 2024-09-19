import socket
import random
import time
import json


class InitialMessage:
    def __init__(self, id, total_epochs):
        self.id = id
        self.total_epochs = total_epochs

    def to_message(self):
        return json.dumps({
           "id": self.id,
           "total_epochs": self.total_epochs
        }).encode()


class StdMessage:
    def __init__(self, message=None, current_epoch=None, total_epochs=None):
        self.message = message
        self.current_epoch = current_epoch
        self.total_epochs = total_epochs

    def to_message(self):
        out = {}
        if self.message is not None:
            out["message"] = self.message
        if self.current_epoch is not None:
            out["current_epoch"] = self.current_epoch
        if self.total_epochs is not None:
            out["total_epochs"] = self.total_epochs

        return json.dumps(out).encode()


class FakeTraining:
    def __init__(self, name):
        self.socket = socket.socket(family=socket.AF_UNIX)
        self.total_epochs = 1000
        self.current_epoch = 0
        self.connected = False
        self.name = name

    def progress(self):
        if not self.connected:
            self.socket.connect("the_socket")
            self.socket.send(
                 InitialMessage(self.name, self.total_epochs).to_message()
            )
            self.connected = True
            return False

        if self.current_epoch == self.total_epochs:
            self.socket.send(b"FINISH")
            return True

        self.current_epoch += random.randint(50, 150)
        self.current_epoch = min(self.current_epoch, self.total_epochs)
        message = f"Hello with {self.current_epoch}"

        total_message = StdMessage(
           message=message,
           current_epoch=self.current_epoch,
           total_epochs=self.total_epochs
        )

        self.socket.send(total_message.to_message())
        return False


all_trainings = [
    FakeTraining("Jordi Wild"),
    FakeTraining("El Bromas"),
    FakeTraining("Tommitllo")
]

while len(all_trainings) != 0:
    time.sleep(0.5)
    index = random.randint(0, len(all_trainings)-1)
    remove = all_trainings[index].progress()
    if remove:
        all_trainings.pop(index)
