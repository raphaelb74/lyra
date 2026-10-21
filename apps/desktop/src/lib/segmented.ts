/** One choice of a `Segmented` control. */
export interface Option<T> {
  value: T;
  /** Short: it sits in a segment. */
  label: string;
  /** One line shown under the setting's name while this is chosen. */
  hint: string;
}
