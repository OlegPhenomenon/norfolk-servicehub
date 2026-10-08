export const APPROVALS = [{ value: 'development_approval', label: 'Development approval' }, { value: 'building_approval', label: 'Building approval' }]
export const MODIFICATION_TYPES = [{ value: 'minor_error', label: 'Minor error, misdescription or miscalculation' }, { value: 'conditions', label: 'Modification to condition(s)' }, { value: 'lapse_date', label: 'Change of approval lapse date' }, { value: 'other', label: 'Any other modification' }]
/** Display name of a DA/BA approval chain type. */
export const approvalLabel = (t: string) => APPROVALS.find(a => a.value === t)?.label ?? t.replaceAll('_', ' ')
