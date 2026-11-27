/** A booking and the table it holds. */
export class Booking {
  constructor(table) {
    this.table = table;
  }

  isFor(table) {
    return this.table === table;
  }
}
