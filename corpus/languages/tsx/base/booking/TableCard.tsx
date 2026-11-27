import { Booking } from "./models";

export function TableCard(props: { booking: Booking }): JSX.Element {
  return <li className="table-card">Table {props.booking.table}</li>;
}
